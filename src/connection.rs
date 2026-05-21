// Copyright 2026 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Wrapping and execution of the `localharness` binary and WebSocket transport.

use std::collections::{HashSet, VecDeque, HashMap};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex, Notify};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::types::{
    BuiltinTools, CapabilitiesConfig, Content, ContentPrimitive, GeminiConfig,
    HarnessConfig, HarnessSideTools, InputEvent, OutputEvent,
    Step, StepSource, StepState, StepStatus, StepTarget, StepType, Tool,
    ToolCall, ToolConfirmation, ToolResponse, TrajectoryState,
    UserQuestionsResponse, QuestionsResponseInner, UserQuestionAnswer,
    FindToolConfig, RunCommandToolConfig, SubagentsConfig, UserQuestionsConfig,
    FileEditToolConfig, ViewFileToolConfig, WriteToFileToolConfig,
    GrepSearchToolConfig, ListDirToolConfig, GenerateImageToolConfig, Workspace,
    FilesystemWorkspace, InitializeConversationEvent,
};
use crate::policy::PolicyEngine;

// =============================================================================
// Discovery of localharness binary
// =============================================================================

pub fn find_localharness() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("ANTIGRAVITY_HARNESS_PATH") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    if let Ok(curr) = std::env::current_dir() {
        let venv_lib = curr.join(".venv").join("lib");
        if venv_lib.exists() {
            if let Ok(entries) = std::fs::read_dir(&venv_lib) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() && path.file_name().unwrap().to_string_lossy().starts_with("python") {
                        let candidate = path.join("site-packages").join("google").join("antigravity").join("bin").join("localharness");
                        if candidate.exists() {
                            return Ok(candidate);
                        }
                    }
                }
            }
        }

        let candidate = curr.join(".venv").join("bin").join("localharness");
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    if let Ok(path_env) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_env) {
            let candidate = dir.join("localharness");
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    Err("Could not find default localharness binary. Set ANTIGRAVITY_HARNESS_PATH or ensure it is installed in your virtual environment.".to_string())
}

pub fn normalize_wire_path(path: &str) -> String {
    if path.starts_with("file://") {
        if let Ok(parsed) = url::Url::parse(path) {
            if parsed.scheme() == "file" {
                if let Ok(decoded) = percent_encoding::percent_decode_str(parsed.path()).decode_utf8() {
                    return decoded.into_owned();
                }
            }
        }
    }
    path.to_string()
}

// =============================================================================
// Tool Context and Custom Tool Trait
// =============================================================================

pub type ToolFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value, String>> + Send>>;

pub trait CustomTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    fn call(&self, args: serde_json::Value, ctx: Option<ToolContext>) -> ToolFuture;
}

#[derive(Clone)]
pub struct ToolContext {
    state: Arc<Mutex<HashMap<String, serde_json::Value>>>,
    sender: mpsc::UnboundedSender<InputEvent>,
    conversation_id: String,
    is_idle_flag: Arc<std::sync::Mutex<bool>>,
}

impl ToolContext {
    pub fn conversation_id(&self) -> String {
        self.conversation_id.clone()
    }

    pub async fn is_idle(&self) -> bool {
        *self.is_idle_flag.lock().unwrap()
    }

    pub async fn send(&self, message: &str) -> Result<(), String> {
        let event = InputEvent {
            user_input: None,
            complex_user_input: None,
            tool_confirmation: None,
            tool_response: None,
            question_response: None,
            halt_request: None,
            automated_trigger: Some(message.to_string()),
        };
        self.sender.send(event).map_err(|e| format!("Failed to send trigger: {}", e))
    }

    pub async fn get_state(&self, key: &str) -> Option<serde_json::Value> {
        let map = self.state.lock().await;
        map.get(key).cloned()
    }

    pub async fn set_state(&self, key: &str, value: serde_json::Value) {
        let mut map = self.state.lock().await;
        map.insert(key.to_string(), value);
    }
}

// =============================================================================
// Connection Manager
// =============================================================================

pub struct Connection {
    child: Mutex<Option<Child>>,
    _stdin: Mutex<Option<tokio::process::ChildStdin>>,
    ws_sender: Mutex<Option<futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, Message>>>,
    step_rx: Mutex<mpsc::UnboundedReceiver<Step>>,
    is_idle: Arc<std::sync::Mutex<bool>>,
    idle_notify: Arc<Notify>,
    conversation_id: Arc<std::sync::Mutex<String>>,
    tool_context: ToolContext,
    parent_idle: Arc<std::sync::Mutex<bool>>,
    active_subagent_ids: Arc<std::sync::Mutex<HashSet<String>>>,
}

impl Connection {
    pub fn conversation_id(&self) -> String {
        self.conversation_id.lock().unwrap().clone()
    }

    pub fn is_idle(&self) -> bool {
        *self.is_idle.lock().unwrap()
    }

    pub async fn wait_for_idle(&self) {
        loop {
            if *self.is_idle.lock().unwrap() {
                break;
            }
            self.idle_notify.notified().await;
        }
    }

    pub async fn send(&self, prompt: Option<Content>) -> Result<(), String> {
        {
            let mut is_idle = self.is_idle.lock().unwrap();
            *is_idle = false;
        }
        {
            let mut parent_idle = self.parent_idle.lock().unwrap();
            *parent_idle = false;
        }
        {
            let mut active_subagents = self.active_subagent_ids.lock().unwrap();
            active_subagents.clear();
        }

        let input_event = match prompt {
            None => InputEvent {
                user_input: Some(String::new()),
                complex_user_input: None,
                tool_confirmation: None,
                tool_response: None,
                question_response: None,
                halt_request: None,
                automated_trigger: None,
            },
            Some(content) => {
                if content.len() == 1 {
                    if let ContentPrimitive::Text(ref txt) = content[0] {
                        InputEvent {
                            user_input: Some(txt.clone()),
                            complex_user_input: None,
                            tool_confirmation: None,
                            tool_response: None,
                            question_response: None,
                            halt_request: None,
                            automated_trigger: None,
                        }
                    } else {
                        self.build_complex_input(content)
                    }
                } else {
                    self.build_complex_input(content)
                }
            }
        };

        let json = serde_json::to_string(&input_event).map_err(|e| e.to_string())?;
        let mut sender = self.ws_sender.lock().await;
        if let Some(ref mut ws) = *sender {
            ws.send(Message::Text(json)).await.map_err(|e| e.to_string())?;
        } else {
            return Err("WebSocket connection is closed".to_string());
        }
        Ok(())
    }

    fn build_complex_input(&self, content: Content) -> InputEvent {
        let parts = content
            .into_iter()
            .map(|c| match c {
                ContentPrimitive::Text(t) => crate::types::UserInputPart {
                    text: Some(t),
                    media: None,
                },
                ContentPrimitive::Media(m) => crate::types::UserInputPart {
                    text: None,
                    media: Some(crate::types::MediaInput {
                        mime_type: m.mime_type,
                        data: m.data,
                        description: m.description,
                    }),
                },
            })
            .collect();

        InputEvent {
            user_input: None,
            complex_user_input: Some(crate::types::UserInput { parts }),
            tool_confirmation: None,
            tool_response: None,
            question_response: None,
            halt_request: None,
            automated_trigger: None,
        }
    }

    pub async fn receive_steps(&self) -> Option<Step> {
        let mut rx = self.step_rx.lock().await;

        if *self.is_idle.lock().unwrap() {
            match rx.try_recv() {
                Ok(step) => {
                    if step.id == "IDLE_SENTINEL" {
                        return None;
                    }
                    return Some(step);
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                    return None;
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    return None;
                }
            }
        }

        while let Some(step) = rx.recv().await {
            if step.id == "IDLE_SENTINEL" {
                return None;
            }
            return Some(step);
        }
        None
    }

    pub async fn cancel(&self) -> Result<(), String> {
        let event = InputEvent {
            user_input: None,
            complex_user_input: None,
            tool_confirmation: None,
            tool_response: None,
            question_response: None,
            halt_request: Some(true),
            automated_trigger: None,
        };
        let json = serde_json::to_string(&event).map_err(|e| e.to_string())?;
        let mut sender = self.ws_sender.lock().await;
        if let Some(ref mut ws) = *sender {
            ws.send(Message::Text(json)).await.map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub async fn send_trigger_notification(&self, message: &str) -> Result<(), String> {
        let event = InputEvent {
            user_input: None,
            complex_user_input: None,
            tool_confirmation: None,
            tool_response: None,
            question_response: None,
            halt_request: None,
            automated_trigger: Some(message.to_string()),
        };
        let json = serde_json::to_string(&event).map_err(|e| e.to_string())?;
        let mut sender = self.ws_sender.lock().await;
        if let Some(ref mut ws) = *sender {
            ws.send(Message::Text(json)).await.map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub async fn disconnect(&self) {
        log::info!("Disconnecting localharness connection");
        let mut sender = self.ws_sender.lock().await;
        if let Some(mut ws) = sender.take() {
            let _ = ws.close().await;
        }

        let mut stdin_guard = self._stdin.lock().await;
        let _ = stdin_guard.take();

        let mut child_guard = self.child.lock().await;
        if let Some(mut child) = child_guard.take() {
            let _ = child.kill().await;
        }
    }

    pub fn tool_context(&self) -> ToolContext {
        self.tool_context.clone()
    }
}

// =============================================================================
// Connection Strategy implementation
// =============================================================================

pub struct LocalConnectionStrategy {
    pub binary_path: PathBuf,
    pub save_dir: String,
    pub gemini_config: GeminiConfig,
    pub system_instructions: Option<crate::types::SystemInstructions>,
    pub capabilities: CapabilitiesConfig,
    pub workspaces: Vec<String>,
    pub app_data_dir: String,
    pub skills_paths: Vec<String>,
    pub policies: Vec<crate::policy::Policy>,
    pub custom_tools: Vec<Arc<dyn CustomTool>>,
}

impl LocalConnectionStrategy {
    pub fn new(save_dir: String) -> Self {
        let binary_path = find_localharness().expect("Harness binary path is missing");
        let workspaces = if let Ok(curr) = std::env::current_dir() {
            vec![curr.to_string_lossy().to_string()]
        } else {
            Vec::new()
        };
        Self {
            binary_path,
            save_dir,
            gemini_config: GeminiConfig::default(),
            system_instructions: None,
            capabilities: CapabilitiesConfig::default(),
            workspaces,
            app_data_dir: String::new(),
            skills_paths: Vec::new(),
            policies: Vec::new(),
            custom_tools: Vec::new(),
        }
    }
}

impl Default for LocalConnectionStrategy {
    fn default() -> Self {
        let duration = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let temp = std::env::temp_dir().join(format!("antigravity_{}_{}", std::process::id(), duration.as_nanos()));
        let _ = std::fs::create_dir_all(&temp);
        Self::new(temp.to_string_lossy().to_string())
    }
}

impl LocalConnectionStrategy {
    pub async fn connect(self) -> Result<Arc<Connection>, String> {
        log::info!("Starting harness connection flow...");

        let mut cmd = Command::new(&self.binary_path);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn localharness: {}", e))?;

        // LE 4-byte handshake
        let mut stdin = child.stdin.take().ok_or("Failed to open stdin")?;
        let mut stdout = child.stdout.take().ok_or("Failed to open stdout")?;
        let stderr = child.stderr.take().ok_or("Failed to open stderr")?;

        let input_bytes = crate::proto::encode_input_config(&self.save_dir);
        let len_bytes = (input_bytes.len() as u32).to_le_bytes();

        stdin.write_all(&len_bytes).await.map_err(|e| e.to_string())?;
        stdin.write_all(&input_bytes).await.map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())?;

        let mut len_buf = [0u8; 4];
        stdout.read_exact(&mut len_buf).await.map_err(|e| format!("Failed to read output length: {}", e))?;
        let out_len = u32::from_le_bytes(len_buf) as usize;
        let mut out_bytes = vec![0u8; out_len];
        stdout.read_exact(&mut out_bytes).await.map_err(|e| format!("Failed to read output config: {}", e))?;

        let output_config = crate::proto::decode_output_config(&out_bytes)?;
        log::info!("Discovered WebSocket server at port {}", output_config.port);

        // Stderr reader thread
        let stderr_lines = Arc::new(Mutex::new(VecDeque::with_capacity(100)));
        let stderr_lines_clone = stderr_lines.clone();
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut reader = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                log::info!("harness stderr: {}", line);
                let mut lines = stderr_lines_clone.lock().await;
                if lines.len() >= 100 {
                    lines.pop_front();
                }
                lines.push_back(line);
            }
        });

        // WS Connection
        let ws_url = format!("ws://localhost:{}/", output_config.port);
        let mut request = ws_url.into_client_request().map_err(|e| e.to_string())?;
        request.headers_mut().insert(
            "x-goog-api-key",
            tokio_tungstenite::tungstenite::http::HeaderValue::from_str(&output_config.api_key)
                .map_err(|e| e.to_string())?
        );

        let mut ws = None;
        let mut backoff = std::time::Duration::from_millis(100);
        for attempt in 0..5 {
            match tokio_tungstenite::connect_async(request.clone()).await {
                Ok((socket, _)) => {
                    ws = Some(socket);
                    break;
                }
                Err(e) => {
                    if attempt == 4 {
                        return Err(format!("Failed to connect to WebSocket: {}", e));
                    }
                    tokio::time::sleep(backoff).await;
                    backoff *= 2;
                }
            }
        }
        let ws = ws.unwrap();
        let (mut ws_write, mut ws_read) = ws.split();

        // 1. Build initial conversation event config
        let tool_protos = self.custom_tools.iter().map(|ct| Tool {
            name: ct.name().to_string(),
            description: ct.description().to_string(),
            parameters_json_schema: serde_json::to_string(&ct.parameters_schema()).unwrap(),
            response_json_schema: None,
        }).collect();

        let all_tools = BuiltinTools::all_tools();
        let active_tools: HashSet<_> = if let Some(ref enabled) = self.capabilities.enabled_tools {
            enabled.iter().cloned().collect()
        } else if let Some(ref disabled) = self.capabilities.disabled_tools {
            let disabled_set: HashSet<_> = disabled.iter().cloned().collect();
            all_tools.into_iter().filter(|t| !disabled_set.contains(t)).collect()
        } else {
            all_tools.into_iter().collect()
        };

        let workspaces_pb = self.workspaces.iter().map(|w| Workspace {
            filesystem_workspace: FilesystemWorkspace {
                directory: normalize_wire_path(w),
            }
        }).collect();

        let subagents_enabled = self.capabilities.enable_subagents && active_tools.contains(&BuiltinTools::StartSubagent);

        let harness_config = HarnessConfig {
            tools: tool_protos,
            system_instructions: self.system_instructions.clone(),
            cascade_id: String::new(),
            gemini_config: Some(self.gemini_config.to_wire()),
            workspaces: workspaces_pb,
            skills_paths: self.skills_paths.clone(),
            harness_side_tools: HarnessSideTools {
                find: FindToolConfig { enabled: active_tools.contains(&BuiltinTools::FindFile) },
                run_command: RunCommandToolConfig { enabled: active_tools.contains(&BuiltinTools::RunCommand) },
                subagents: SubagentsConfig { enabled: subagents_enabled },
                user_questions: UserQuestionsConfig { enabled: active_tools.contains(&BuiltinTools::AskQuestion) },
                file_edit: FileEditToolConfig { enabled: active_tools.contains(&BuiltinTools::EditFile) },
                view_file: ViewFileToolConfig { enabled: active_tools.contains(&BuiltinTools::ViewFile) },
                write_to_file: WriteToFileToolConfig { enabled: active_tools.contains(&BuiltinTools::CreateFile) },
                grep_search: GrepSearchToolConfig { enabled: active_tools.contains(&BuiltinTools::SearchDir) },
                list_dir: ListDirToolConfig { enabled: active_tools.contains(&BuiltinTools::ListDir) },
                generate_image: GenerateImageToolConfig {
                    enabled: active_tools.contains(&BuiltinTools::GenerateImage),
                    model_name: Some(self.capabilities.image_model.clone()),
                },
            },
            compaction_threshold: self.capabilities.compaction_threshold.unwrap_or(0),
            finish_tool_schema_json: self.capabilities.finish_tool_schema_json.clone().unwrap_or_default(),
            app_data_dir: self.app_data_dir.clone(),
        };

        let init_event = InitializeConversationEvent {
            config: harness_config,
        };

        let init_json = serde_json::to_string(&init_event).unwrap();
        ws_write.send(Message::Text(init_json)).await.map_err(|e| e.to_string())?;

        // Setup channel queues for steps
        let (step_tx, step_rx) = mpsc::unbounded_channel();
        let (input_tx, mut input_rx) = mpsc::unbounded_channel::<InputEvent>();

        let is_idle = Arc::new(std::sync::Mutex::new(true));
        let idle_notify = Arc::new(Notify::new());
        let conversation_id = Arc::new(std::sync::Mutex::new(String::new()));

        let connection = Arc::new(Connection {
            child: Mutex::new(Some(child)),
            _stdin: Mutex::new(Some(stdin)),
            ws_sender: Mutex::new(Some(ws_write)),
            step_rx: Mutex::new(step_rx),
            is_idle: is_idle.clone(),
            idle_notify: idle_notify.clone(),
            conversation_id: conversation_id.clone(),
            tool_context: ToolContext {
                state: Arc::new(Mutex::new(HashMap::new())),
                sender: input_tx.clone(),
                conversation_id: String::new(),
                is_idle_flag: is_idle.clone(),
            },
            parent_idle: Arc::new(std::sync::Mutex::new(true)),
            active_subagent_ids: Arc::new(std::sync::Mutex::new(HashSet::new())),
        });

        // WS Writer Task
        let connection_writer = connection.clone();
        tokio::spawn(async move {
            while let Some(event) = input_rx.recv().await {
                if let Ok(json) = serde_json::to_string(&event) {
                    let mut ws_sender = connection_writer.ws_sender.lock().await;
                    if let Some(ref mut ws) = *ws_sender {
                        let _ = ws.send(Message::Text(json)).await;
                    }
                }
            }
        });

        let mut final_policies = self.policies.clone();
        if final_policies.is_empty() {
            final_policies = crate::policy::confirm_run_command(None);
        }

        if !self.workspaces.is_empty() {
            let app_data_path = if self.app_data_dir.is_empty() {
                if let Ok(home) = std::env::var("HOME") {
                    PathBuf::from(home).join(".gemini").join("antigravity")
                } else {
                    PathBuf::from("~/.gemini/antigravity")
                }
            } else {
                PathBuf::from(&self.app_data_dir)
            };

            let app_data_path_abs = if app_data_path.is_absolute() {
                app_data_path
            } else if let Ok(curr) = std::env::current_dir() {
                curr.join(app_data_path)
            } else {
                app_data_path
            };

            let mut allowed_paths = self.workspaces.clone();
            allowed_paths.push(app_data_path_abs.to_string_lossy().to_string());

            let workspace_policies = crate::policy::workspace_only(allowed_paths);
            let mut prepended = workspace_policies;
            prepended.extend(final_policies);
            final_policies = prepended;
        }

        // WS Reader Task
        let policies_arc = Arc::new(PolicyEngine::new(final_policies));
        let custom_tools_arc = Arc::new(self.custom_tools);
        let connection_reader = connection.clone();
        let conversation_id_clone = conversation_id.clone();
        let is_idle_clone = is_idle.clone();
        let idle_notify_clone = idle_notify.clone();
        let parent_idle_clone = connection.parent_idle.clone();
        let active_subagent_ids_clone = connection.active_subagent_ids.clone();
        let step_tx_clone = step_tx.clone();

        tokio::spawn(async move {
            while let Some(msg_res) = ws_read.next().await {
                let msg = match msg_res {
                    Ok(Message::Text(t)) => t,
                    _ => break,
                };

                let event: OutputEvent = match serde_json::from_str(&msg) {
                    Ok(e) => e,
                    Err(e) => {
                        log::error!("Failed to parse WebSocket JSON event: {}. Raw: {}", e, msg);
                        continue;
                    }
                };

                if let Some(step_update) = event.step_update {
                    // Update Cascade/Conversation ID if set
                    if let Some(ref cid) = step_update.cascade_id {
                        if let Some(ref tid) = step_update.trajectory_id {
                            if cid == tid {
                                let mut id_guard = conversation_id_clone.lock().unwrap();
                                *id_guard = cid.clone();
                            }
                        }
                    }

                    // Map StepUpdate to Step
                    let mut step = Step::default();
                    let traj_id = step_update.trajectory_id.clone().unwrap_or_default();
                    let step_idx = step_update.step_index;

                    step.id = if !traj_id.is_empty() {
                        format!("{}:{}", traj_id, step_idx)
                    } else {
                        step_idx.to_string()
                    };
                    step.step_index = step_idx;
                    step.source = step_update.source;
                    step.target = step_update.target;

                    step.status = match step_update.state {
                        StepState::Active => StepStatus::Active,
                        StepState::Done => StepStatus::Done,
                        StepState::WaitingForUser => StepStatus::WaitingForUser,
                        StepState::Error => StepStatus::Error,
                        _ => StepStatus::Unknown,
                    };

                    step.content = step_update.text.clone().unwrap_or_default();
                    step.content_delta = step_update.text_delta.clone().unwrap_or_default();
                    step.thinking = step_update.thinking.clone().unwrap_or_default();
                    step.thinking_delta = step_update.thinking_delta.clone().unwrap_or_default();
                    step.usage_metadata = event.usage_metadata.clone();

                    // Determine StepType
                    step.r#type = if step_update.compaction.is_some() {
                        StepType::Compaction
                    } else if step_update.finish.is_some() {
                        StepType::Finish
                    } else if step_update.list_directory.is_some()
                        || step_update.find_file.is_some()
                        || step_update.search_directory.is_some()
                        || step_update.view_file.is_some()
                        || step_update.create_file.is_some()
                        || step_update.edit_file.is_some()
                        || step_update.run_command.is_some()
                        || step_update.invoke_subagent.is_some()
                        || step_update.generate_image.is_some()
                    {
                        StepType::ToolCall
                    } else if !step.content.is_empty() {
                        StepType::TextResponse
                    } else {
                        StepType::Unknown
                    };

                    // Extract tool result or details
                    if step.r#type == StepType::Finish {
                        if let Some(ref finish_val) = step_update.finish {
                            if let Some(out_str) = finish_val.get("output_string").and_then(|v| v.as_str()) {
                                if let Ok(parsed_out) = serde_json::from_str(out_str) {
                                    step.structured_output = Some(parsed_out);
                                }
                            }
                        }
                    }

                    // Extract ToolCall
                    let mut active_tool_pair = None;
                    let builtin_fields = vec![
                        (BuiltinTools::ListDir, "list_directory"),
                        (BuiltinTools::SearchDir, "search_directory"),
                        (BuiltinTools::FindFile, "find_file"),
                        (BuiltinTools::ViewFile, "view_file"),
                        (BuiltinTools::CreateFile, "create_file"),
                        (BuiltinTools::EditFile, "edit_file"),
                        (BuiltinTools::RunCommand, "run_command"),
                        (BuiltinTools::AskQuestion, "questions_request"),
                        (BuiltinTools::StartSubagent, "invoke_subagent"),
                        (BuiltinTools::GenerateImage, "generate_image"),
                        (BuiltinTools::Finish, "finish"),
                    ];

                    for (tool_enum, field) in builtin_fields {
                        if let Some(sub_msg) = match field {
                            "list_directory" => &step_update.list_directory,
                            "search_directory" => &step_update.search_directory,
                            "find_file" => &step_update.find_file,
                            "view_file" => &step_update.view_file,
                            "create_file" => &step_update.create_file,
                            "edit_file" => &step_update.edit_file,
                            "run_command" => &step_update.run_command,
                            "invoke_subagent" => &step_update.invoke_subagent,
                            "generate_image" => &step_update.generate_image,
                            "finish" => &step_update.finish,
                            _ => &None,
                        } {
                            active_tool_pair = Some((tool_enum.as_str().to_string(), sub_msg.clone()));
                            break;
                        }
                    }

                    if let Some((tool_name, mut args)) = active_tool_pair {
                        let mut canonical_path = None;
                        if let Some(obj) = args.as_object_mut() {
                            for path_key in &["path", "file_path", "TargetFile", "directory_path"] {
                                if let Some(val) = obj.get(*path_key).and_then(|v| v.as_str()) {
                                    let normalized = normalize_wire_path(val);
                                    obj.insert(path_key.to_string(), serde_json::Value::String(normalized.clone()));
                                    canonical_path = Some(normalized);
                                }
                            }
                        }

                        let tc = ToolCall {
                            id: Some(step.id.clone()),
                            name: tool_name,
                            args,
                            arguments_json: None,
                            canonical_path,
                        };
                        step.tool_calls = vec![tc];
                    }

                    let _ = step_tx.send(step);

                    // Handle wait requests (tool confirmation & questions)
                    if step_update.state == StepState::WaitingForUser {
                        if let Some(ref tool_conf_req) = step_update.tool_confirmation_request {
                            // Extract tool name and arguments from wait request fields
                            let mut action_str = "unknown".to_string();
                            let mut args = tool_conf_req.clone();
                            let mut found_action = false;

                            let builtin_fields = vec![
                                (BuiltinTools::ListDir, "list_directory"),
                                (BuiltinTools::SearchDir, "search_directory"),
                                (BuiltinTools::FindFile, "find_file"),
                                (BuiltinTools::ViewFile, "view_file"),
                                (BuiltinTools::CreateFile, "create_file"),
                                (BuiltinTools::EditFile, "edit_file"),
                                (BuiltinTools::RunCommand, "run_command"),
                                (BuiltinTools::StartSubagent, "invoke_subagent"),
                                (BuiltinTools::GenerateImage, "generate_image"),
                                (BuiltinTools::Finish, "finish"),
                            ];

                            for (tool_enum, field) in builtin_fields {
                                let field_val = match field {
                                    "list_directory" => &step_update.list_directory,
                                    "search_directory" => &step_update.search_directory,
                                    "find_file" => &step_update.find_file,
                                    "view_file" => &step_update.view_file,
                                    "create_file" => &step_update.create_file,
                                    "edit_file" => &step_update.edit_file,
                                    "run_command" => &step_update.run_command,
                                    "invoke_subagent" => &step_update.invoke_subagent,
                                    "generate_image" => &step_update.generate_image,
                                    "finish" => &step_update.finish,
                                    _ => &None,
                                };
                                if let Some(sub_msg) = field_val {
                                    action_str = tool_enum.as_str().to_string();
                                    args = sub_msg.clone();
                                    found_action = true;
                                    break;
                                }
                            }

                            if !found_action {
                                action_str = "pre_request_host_tool_request".to_string();
                            }

                            let mut canonical_path = None;
                            if let Some(obj) = args.as_object_mut() {
                                if let Some(req_txt) = &step_update.request_text {
                                    obj.insert("request_text".to_string(), serde_json::Value::String(req_txt.clone()));
                                }
                                for path_key in &["path", "file_path", "TargetFile", "directory_path"] {
                                    if let Some(val) = obj.get(*path_key).and_then(|v| v.as_str()) {
                                        let normalized = normalize_wire_path(val);
                                        obj.insert(path_key.to_string(), serde_json::Value::String(normalized.clone()));
                                        canonical_path = Some(normalized);
                                    }
                                }
                            }

                            let tc = ToolCall {
                                id: Some(format!("{}:{}", traj_id, step_idx)),
                                name: action_str.clone(),
                                args,
                                arguments_json: None,
                                canonical_path,
                            };

                            let allowed = if action_str == "pre_request_host_tool_request" {
                                true
                            } else {
                                match policies_arc.evaluate(&tc) {
                                    Ok(approved) => approved,
                                    Err(err) => {
                                        log::warn!("Tool Call Policy denied: {}", err);
                                        false
                                    }
                                }
                            };

                            let conf_response = InputEvent {
                                user_input: None,
                                complex_user_input: None,
                                tool_confirmation: Some(ToolConfirmation {
                                    trajectory_id: traj_id.clone(),
                                    step_index: step_idx,
                                    accepted: allowed,
                                }),
                                tool_response: None,
                                question_response: None,
                                halt_request: None,
                                automated_trigger: None,
                            };
                            let _ = input_tx.send(conf_response);
                        }

                        if let Some(_) = step_update.questions_request {
                            // Automatically skip/unanswer questions to prevent deadlocks
                            let q_response = InputEvent {
                                user_input: None,
                                complex_user_input: None,
                                tool_confirmation: None,
                                tool_response: None,
                                question_response: Some(UserQuestionsResponse {
                                    trajectory_id: traj_id.clone(),
                                    step_index: step_idx,
                                    response: QuestionsResponseInner {
                                        answers: vec![UserQuestionAnswer {
                                            unanswered: true,
                                            multiple_choice_answer: None,
                                        }],
                                    },
                                }),
                                halt_request: None,
                                automated_trigger: None,
                            };
                            let _ = input_tx.send(q_response);
                        }
                    }
                } else if let Some(tsu) = event.trajectory_state_update {
                    let main_id = conversation_id_clone.lock().unwrap().clone();
                    let is_subagent = !main_id.is_empty() && tsu.trajectory_id != main_id;

                    if tsu.state == TrajectoryState::Running {
                        if is_subagent {
                            let mut active_ids = active_subagent_ids_clone.lock().unwrap();
                            active_ids.insert(tsu.trajectory_id.clone());
                        }
                    } else if tsu.state == TrajectoryState::Idle {
                        if is_subagent {
                            let mut active_ids = active_subagent_ids_clone.lock().unwrap();
                            active_ids.remove(&tsu.trajectory_id);
                        } else {
                            let mut p_idle = parent_idle_clone.lock().unwrap();
                            *p_idle = true;
                        }

                        let p_idle = *parent_idle_clone.lock().unwrap();
                        let active_empty = active_subagent_ids_clone.lock().unwrap().is_empty();

                        if p_idle && active_empty {
                            let mut is_idle = is_idle_clone.lock().unwrap();
                            *is_idle = true;
                            idle_notify_clone.notify_waiters();

                            // Push the IDLE sentinel!
                            let mut sentinel = Step::default();
                            sentinel.id = "IDLE_SENTINEL".to_string();
                            let _ = step_tx_clone.send(sentinel);
                        }
                    }
                } else if let Some(tc) = event.tool_call {
                    let tc_id = tc.id.clone().unwrap_or_default();
                    let name = tc.name.clone();

                    let args: serde_json::Value = serde_json::from_str(&tc.arguments_json.clone().unwrap_or_default()).unwrap_or(serde_json::Value::Null);
                    let mut matched_tool = None;
                    for ct in custom_tools_arc.iter() {
                        if ct.name() == name {
                            matched_tool = Some(ct.clone());
                            break;
                        }
                    }

                    if let Some(tool) = matched_tool {
                        let tool_call_step = Step {
                            id: tc_id.clone(),
                            step_index: 1,
                            r#type: StepType::ToolCall,
                            source: StepSource::Model,
                            target: StepTarget::Environment,
                            status: StepStatus::Active,
                            tool_calls: vec![ToolCall {
                                id: Some(tc_id.clone()),
                                name: name.clone(),
                                args: args.clone(),
                                arguments_json: None,
                                canonical_path: None,
                            }],
                            ..Default::default()
                        };
                        let _ = step_tx.send(tool_call_step);

                        let input_tx_clone = input_tx.clone();
                        let tc_ctx = connection_reader.tool_context();

                        tokio::spawn(async move {
                            log::info!("Executing custom tool: {}", name);
                            let result = tool.call(args, Some(tc_ctx)).await;
                            let output_val = match result {
                                Ok(val) => {
                                    if val.is_object() {
                                        val
                                    } else {
                                        serde_json::json!({ "result": val })
                                    }
                                }
                                Err(err) => serde_json::json!({ "error": err }),
                            };

                            let resp = ToolResponse {
                                id: tc_id,
                                response_json: serde_json::to_string(&output_val).unwrap(),
                            };

                            let event = InputEvent {
                                user_input: None,
                                complex_user_input: None,
                                tool_confirmation: None,
                                tool_response: Some(resp),
                                question_response: None,
                                halt_request: None,
                                automated_trigger: None,
                            };
                            let _ = input_tx_clone.send(event);
                        });
                    } else {
                        log::warn!("Tool call received but no matching custom tool registered for: {}", name);
                    }
                }
            }
        });

        Ok(connection)
    }
}
