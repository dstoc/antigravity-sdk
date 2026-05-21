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
    ToolCall, ToolConfirmation, TrajectoryState,
    UserQuestionsResponse, QuestionsResponseInner,
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
    pub step_tx: mpsc::UnboundedSender<Step>,
    step_rx: Mutex<mpsc::UnboundedReceiver<Step>>,
    is_idle: Arc<std::sync::Mutex<bool>>,
    idle_notify: Arc<Notify>,
    conversation_id: Arc<std::sync::Mutex<String>>,
    tool_context: ToolContext,
    parent_idle: Arc<std::sync::Mutex<bool>>,
    active_subagent_ids: Arc<std::sync::Mutex<HashSet<String>>>,
    pub hook_runner: Arc<crate::hooks::HookRunner>,
    pub current_turn_context: Arc<std::sync::Mutex<Option<crate::hooks::TurnContext>>>,
    pub pending_builtin_tool_calls: Arc<std::sync::Mutex<HashMap<(String, u32), (ToolCall, crate::hooks::OperationContext)>>>,
    pub subagent_responses: Arc<std::sync::Mutex<HashMap<String, String>>>,
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
        let (hook_res, turn_ctx) = self.hook_runner.dispatch_pre_turn(&prompt).await?;
        {
            let mut guard = self.current_turn_context.lock().unwrap();
            *guard = Some(turn_ctx);
        }

        if !hook_res.allow {
            log::info!("PreTurn hook denied prompt execution: {}", hook_res.message);
            // Push an error step
            let mut err_step = Step::default();
            err_step.id = "PRE_TURN_DENIED".to_string();
            err_step.r#type = StepType::Unknown;
            err_step.source = StepSource::System;
            err_step.status = StepStatus::Error;
            err_step.error = hook_res.message.clone();
            let _ = self.step_tx.send(err_step);

            // Set is_idle to true and notify
            {
                let mut is_idle = self.is_idle.lock().unwrap();
                *is_idle = true;
            }
            self.idle_notify.notify_waiters();

            // Push the IDLE sentinel!
            let mut sentinel = Step::default();
            sentinel.id = "IDLE_SENTINEL".to_string();
            let _ = self.step_tx.send(sentinel);

            return Err(format!("PreTurn hook denied: {}", hook_res.message));
        }

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

        let step_opt = if *self.is_idle.lock().unwrap() {
            match rx.try_recv() {
                Ok(step) => {
                    if step.id == "IDLE_SENTINEL" {
                        None
                    } else {
                        Some(step)
                    }
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => None,
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => None,
            }
        } else {
            match rx.recv().await {
                Some(step) => {
                    if step.id == "IDLE_SENTINEL" {
                        None
                    } else {
                        Some(step)
                    }
                }
                None => None,
            }
        };

        if let Some(ref step) = step_opt {
            if step.source == StepSource::Model
                && (step.status == StepStatus::Done || step.status == StepStatus::Error)
                && step.target == StepTarget::User
            {
                let turn_ctx_opt = self.current_turn_context.lock().unwrap().clone();
                if let Some(turn_ctx) = turn_ctx_opt {
                    let hook_runner_clone = self.hook_runner.clone();
                    let response_content = step.content.clone();
                    tokio::spawn(async move {
                        let _ = hook_runner_clone.dispatch_post_turn(&turn_ctx, &response_content).await;
                    });
                }
            }
        }

        step_opt
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
        let _ = self.hook_runner.dispatch_session_end().await;
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
    pub hook_runner: crate::hooks::HookRunner,
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
            hook_runner: crate::hooks::HookRunner::new(),
        }
    }

    pub fn register_on_session_start<H: crate::hooks::OnSessionStart + 'static>(mut self, hook: H) -> Self {
        self.hook_runner.register_on_session_start(hook);
        self
    }

    pub fn register_on_session_end<H: crate::hooks::OnSessionEnd + 'static>(mut self, hook: H) -> Self {
        self.hook_runner.register_on_session_end(hook);
        self
    }

    pub fn register_pre_turn<H: crate::hooks::PreTurn + 'static>(mut self, hook: H) -> Self {
        self.hook_runner.register_pre_turn(hook);
        self
    }

    pub fn register_post_turn<H: crate::hooks::PostTurn + 'static>(mut self, hook: H) -> Self {
        self.hook_runner.register_post_turn(hook);
        self
    }

    pub fn register_pre_tool_call_decide<H: crate::hooks::PreToolCallDecide + 'static>(mut self, hook: H) -> Self {
        self.hook_runner.register_pre_tool_call_decide(hook);
        self
    }

    pub fn register_post_tool_call<H: crate::hooks::PostToolCall + 'static>(mut self, hook: H) -> Self {
        self.hook_runner.register_post_tool_call(hook);
        self
    }

    pub fn register_on_tool_error<H: crate::hooks::OnToolError + 'static>(mut self, hook: H) -> Self {
        self.hook_runner.register_on_tool_error(hook);
        self
    }

    pub fn register_on_interaction<H: crate::hooks::OnInteraction + 'static>(mut self, hook: H) -> Self {
        self.hook_runner.register_on_interaction(hook);
        self
    }

    pub fn register_on_compaction<H: crate::hooks::OnCompaction + 'static>(mut self, hook: H) -> Self {
        self.hook_runner.register_on_compaction(hook);
        self
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

        let mut hook_runner = self.hook_runner;
        let policies_arc = Arc::new(PolicyEngine::new(final_policies));
        hook_runner.register_pre_tool_call_decide(policies_arc.clone());

        // Dispatch session start hook!
        if let Err(e) = hook_runner.dispatch_session_start().await {
            return Err(format!("Session Start hook failed: {}", e));
        }

        let hook_runner = Arc::new(hook_runner);
        let current_turn_context = Arc::new(std::sync::Mutex::new(None));
        let pending_builtin_tool_calls = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let subagent_responses = Arc::new(std::sync::Mutex::new(HashMap::new()));

        let connection = Arc::new(Connection {
            child: Mutex::new(Some(child)),
            _stdin: Mutex::new(Some(stdin)),
            ws_sender: Mutex::new(Some(ws_write)),
            step_tx: step_tx.clone(),
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
            hook_runner: hook_runner.clone(),
            current_turn_context: current_turn_context.clone(),
            pending_builtin_tool_calls: pending_builtin_tool_calls.clone(),
            subagent_responses: subagent_responses.clone(),
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

        // WS Reader Task
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

                let main_id = conversation_id_clone.lock().unwrap().clone();
                let turn_ctx = {
                    let guard = connection_reader.current_turn_context.lock().unwrap();
                    guard.clone().unwrap_or_else(|| {
                        crate::hooks::TurnContext::new(&connection_reader.hook_runner.session_context)
                    })
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

                    let _ = step_tx.send(step.clone());

                    // Track/handle built-in tool completion
                    let step_key = (traj_id.clone(), step_idx);
                    if step_update.state == StepState::Done {
                        let popped = {
                            let mut guard = connection_reader.pending_builtin_tool_calls.lock().unwrap();
                            guard.remove(&step_key)
                        };
                        if let Some((tc, op_ctx)) = popped {
                            let hook_runner_clone = connection_reader.hook_runner.clone();
                            let extracted = extract_builtin_tool_result(&step_update);
                            let result = crate::types::ToolResult {
                                name: tc.name.clone(),
                                id: tc.id.clone(),
                                result: extracted.unwrap_or_else(|| serde_json::Value::String(step.content.clone())),
                                error: None,
                            };
                            tokio::spawn(async move {
                                let _ = hook_runner_clone.dispatch_post_tool_call(&op_ctx, &result).await;
                            });
                        }
                    } else if step_update.state == StepState::Error {
                        let popped = {
                            let mut guard = connection_reader.pending_builtin_tool_calls.lock().unwrap();
                            guard.remove(&step_key)
                        };
                        if let Some((_tc, op_ctx)) = popped {
                            let hook_runner_clone = connection_reader.hook_runner.clone();
                            let error_msg = step_update.error_message.clone().unwrap_or_else(|| "Built-in tool failed".to_string());
                            let error = std::io::Error::new(std::io::ErrorKind::Other, error_msg);
                            tokio::spawn(async move {
                                let _ = hook_runner_clone.dispatch_on_tool_error(&op_ctx, &error).await;
                            });
                        }
                    }

                    // Track subagent responses
                    let is_subagent_step = !main_id.is_empty() && traj_id != main_id;
                    if is_subagent_step && step.source == StepSource::Model && !step.content.is_empty() {
                        connection_reader.subagent_responses.lock().unwrap().insert(traj_id.clone(), step.content.clone());
                    }

                    // Handle OnCompaction hook
                    if step.r#type == StepType::Compaction {
                        if let Some(ref comp_val) = step_update.compaction {
                            let hook_runner_clone = connection_reader.hook_runner.clone();
                            let turn_ctx_clone = turn_ctx.clone();
                            let comp_val_clone = comp_val.clone();
                            tokio::spawn(async move {
                                let _ = hook_runner_clone.dispatch_compaction(&turn_ctx_clone, &comp_val_clone).await;
                            });
                        }
                    }

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

                            let hook_runner_clone = connection_reader.hook_runner.clone();
                            let input_tx_clone = input_tx.clone();
                            let traj_id_clone = traj_id.clone();
                            let step_idx_clone = step_idx;
                            let tc_clone = tc.clone();
                            let turn_ctx_clone = turn_ctx.clone();
                            let pending_builtin_tool_calls_clone = connection_reader.pending_builtin_tool_calls.clone();

                            tokio::spawn(async move {
                                let (hook_res, _, op_ctx) = if tc_clone.name == "pre_request_host_tool_request" {
                                    (crate::hooks::HookResult::allow(), tc_clone.clone(), crate::hooks::OperationContext::new(&turn_ctx_clone))
                                } else {
                                    match hook_runner_clone.dispatch_pre_tool_call(&turn_ctx_clone, &tc_clone).await {
                                        Ok(res) => res,
                                        Err(err) => (crate::hooks::HookResult::deny(&err), tc_clone.clone(), crate::hooks::OperationContext::new(&turn_ctx_clone)),
                                    }
                                };

                                if hook_res.allow && tc_clone.name != "pre_request_host_tool_request" {
                                    let pending_key = (traj_id_clone.clone(), step_idx_clone);
                                    let pending_val = (tc_clone, op_ctx);
                                    pending_builtin_tool_calls_clone.lock().unwrap().insert(pending_key, pending_val);
                                }

                                let conf_response = InputEvent {
                                    user_input: None,
                                    complex_user_input: None,
                                    tool_confirmation: Some(ToolConfirmation {
                                        trajectory_id: traj_id_clone,
                                        step_index: step_idx_clone,
                                        accepted: hook_res.allow,
                                    }),
                                    tool_response: None,
                                    question_response: None,
                                    halt_request: None,
                                    automated_trigger: None,
                                };
                                let _ = input_tx_clone.send(conf_response);
                            });
                        }

                        if let Some(questions_req) = step_update.questions_request {
                            let hook_runner_clone = connection_reader.hook_runner.clone();
                            let input_tx_clone = input_tx.clone();
                            let traj_id_clone = traj_id.clone();
                            let step_idx_clone = step_idx;
                            let turn_ctx_clone = turn_ctx.clone();

                            tokio::spawn(async move {
                                let mut questions_list = Vec::new();
                                let mut indices_to_hook = Vec::new();
                                for (i, uq) in questions_req.questions.iter().enumerate() {
                                    let mc = &uq.multiple_choice;
                                    {
                                        let opts = mc.choices.iter().enumerate().map(|(j, choice)| {
                                            crate::types::AskQuestionOption {
                                                id: (j + 1).to_string(),
                                                text: choice.clone(),
                                            }
                                        }).collect();
                                        questions_list.push(crate::types::AskQuestionEntry {
                                            question: mc.question.clone(),
                                            options: opts,
                                            is_multi_select: mc.is_multi_select,
                                        });
                                        indices_to_hook.push(i);
                                    }
                                }

                                let mut answers = vec![
                                    crate::types::UserQuestionAnswer {
                                        unanswered: true,
                                        multiple_choice_answer: None,
                                    };
                                    questions_req.questions.len()
                                ];

                                if !questions_list.is_empty() {
                                    let spec = crate::types::AskQuestionInteractionSpec { questions: questions_list };
                                    if let Ok((hook_res, Some(question_res), _op_ctx)) = hook_runner_clone.dispatch_interaction(&turn_ctx_clone, &spec).await {
                                        if hook_res.allow {
                                            for (orig_idx, r) in indices_to_hook.into_iter().zip(question_res.response.answers.into_iter()) {
                                                if orig_idx < answers.len() {
                                                    answers[orig_idx] = r;
                                                }
                                            }
                                        }
                                    }
                                }

                                let q_response = InputEvent {
                                    user_input: None,
                                    complex_user_input: None,
                                    tool_confirmation: None,
                                    tool_response: None,
                                    question_response: Some(UserQuestionsResponse {
                                        trajectory_id: traj_id_clone,
                                        step_index: step_idx_clone,
                                        response: QuestionsResponseInner { answers },
                                    }),
                                    halt_request: None,
                                    automated_trigger: None,
                                };
                                let _ = input_tx_clone.send(q_response);
                            });
                        }
                    }
                } else if let Some(tsu) = event.trajectory_state_update {
                    let is_subagent = !main_id.is_empty() && tsu.trajectory_id != main_id;

                    if tsu.state == TrajectoryState::Running {
                        if is_subagent {
                            let mut active_ids = active_subagent_ids_clone.lock().unwrap();
                            active_ids.insert(tsu.trajectory_id.clone());
                        }
                    } else if tsu.state == TrajectoryState::Idle {
                        if is_subagent {
                            active_subagent_ids_clone.lock().unwrap().remove(&tsu.trajectory_id);

                            // Dispatch PostToolCall for subagent!
                            let hook_runner_clone = connection_reader.hook_runner.clone();
                            let subagent_responses_clone = connection_reader.subagent_responses.clone();
                            let response_text = subagent_responses_clone.lock().unwrap().remove(&tsu.trajectory_id).unwrap_or_default();

                            let op_ctx = crate::hooks::OperationContext::new(&turn_ctx);
                            let result = crate::types::ToolResult {
                                name: crate::types::BuiltinTools::StartSubagent.as_str().to_string(),
                                id: Some(tsu.trajectory_id.clone()),
                                result: serde_json::Value::String(response_text),
                                error: None,
                            };
                            tokio::spawn(async move {
                                let _ = hook_runner_clone.dispatch_post_tool_call(&op_ctx, &result).await;
                            });
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
                        let hook_runner_clone = connection_reader.hook_runner.clone();
                        let input_tx_clone = input_tx.clone();
                        let tc_ctx = connection_reader.tool_context();
                        let tc_id_clone = tc_id.clone();
                        let name_clone = name.clone();
                        let args_clone = args.clone();
                        let step_tx_clone = step_tx.clone();
                        let turn_ctx_clone = turn_ctx.clone();

                        tokio::spawn(async move {
                            let tool_call_step = Step {
                                id: tc_id_clone.clone(),
                                step_index: 1,
                                r#type: StepType::ToolCall,
                                source: StepSource::Model,
                                target: StepTarget::Environment,
                                status: StepStatus::Active,
                                tool_calls: vec![ToolCall {
                                    id: Some(tc_id_clone.clone()),
                                    name: name_clone.clone(),
                                    args: args_clone.clone(),
                                    arguments_json: None,
                                    canonical_path: None,
                                }],
                                ..Default::default()
                            };
                            let _ = step_tx_clone.send(tool_call_step);

                            let tc_struct = ToolCall {
                                id: Some(tc_id_clone.clone()),
                                name: name_clone.clone(),
                                args: args_clone.clone(),
                                arguments_json: None,
                                canonical_path: None,
                            };

                            // Dispatch PreToolCallDecide for custom tools
                            let (hook_res, _, op_ctx) = match hook_runner_clone.dispatch_pre_tool_call(&turn_ctx_clone, &tc_struct).await {
                                Ok(res) => res,
                                Err(err) => (crate::hooks::HookResult::deny(&err), tc_struct.clone(), crate::hooks::OperationContext::new(&turn_ctx_clone)),
                            };

                            if !hook_res.allow {
                                let reason = if hook_res.message.is_empty() { "No reason provided".to_string() } else { hook_res.message };
                                let err_msg = format!("Tool execution denied by hook policy: {}", reason);
                                send_tool_response(&input_tx_clone, &tc_id_clone, serde_json::json!({ "error": err_msg }));
                                return;
                            }

                            log::info!("Executing custom tool: {}", name_clone);
                            let result = tool.call(args_clone, Some(tc_ctx)).await;

                            let final_result = match result {
                                Ok(val) => {
                                    let output_val = if val.is_object() {
                                        val
                                    } else {
                                        serde_json::json!({ "result": val })
                                    };
                                    // Dispatch PostToolCall on success
                                    let tool_res = crate::types::ToolResult {
                                        name: name_clone.clone(),
                                        id: Some(tc_id_clone.clone()),
                                        result: output_val.clone(),
                                        error: None,
                                    };
                                    let _ = hook_runner_clone.dispatch_post_tool_call(&op_ctx, &tool_res).await;
                                    Ok(output_val)
                                }
                                Err(err) => {
                                    // Dispatch OnToolError on failure
                                    let error = std::io::Error::new(std::io::ErrorKind::Other, err.clone());
                                    match hook_runner_clone.dispatch_on_tool_error(&op_ctx, &error).await {
                                        Ok((rec_res, Some(recovery_val))) if rec_res.allow => {
                                            // Recovered: dispatch PostToolCall for recovered value
                                            let tool_res = crate::types::ToolResult {
                                                name: name_clone.clone(),
                                                id: Some(tc_id_clone.clone()),
                                                result: recovery_val.clone(),
                                                error: None,
                                            };
                                            let _ = hook_runner_clone.dispatch_post_tool_call(&op_ctx, &tool_res).await;
                                            Ok(recovery_val)
                                        }
                                        _ => Err(err)
                                    }
                                }
                            };

                            let response_json = match final_result {
                                Ok(val) => val,
                                Err(err) => serde_json::json!({ "error": err }),
                            };

                            send_tool_response(&input_tx_clone, &tc_id_clone, response_json);
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

fn extract_builtin_tool_result(step_update: &crate::types::StepUpdate) -> Option<serde_json::Value> {
    if let Some(ref rc) = step_update.run_command {
        if let Some(out) = rc.get("combined_output").or_else(|| rc.get("combinedOutput")).and_then(|v| v.as_str()) {
            return Some(serde_json::json!({ "output": out }));
        }
    }
    if let Some(ref ld) = step_update.list_directory {
        if let Some(results) = ld.get("results").and_then(|v| v.as_array()) {
            let entries: Vec<serde_json::Value> = results.iter().map(|r| {
                serde_json::json!({
                    "name": r.get("name").and_then(|v| v.as_str()).unwrap_or_default(),
                    "is_directory": r.get("is_directory").or_else(|| r.get("isDirectory")).and_then(|v| v.as_bool()).unwrap_or_default(),
                    "file_size": r.get("file_size").or_else(|| r.get("fileSize")).and_then(|v| v.as_u64()).unwrap_or_default(),
                })
            }).collect();
            return Some(serde_json::json!({ "entries": entries }));
        }
    }
    if let Some(ref ff) = step_update.find_file {
        if let Some(out) = ff.get("output").and_then(|v| v.as_str()) {
            return Some(serde_json::json!({ "output": out }));
        }
    }
    if let Some(ref sd) = step_update.search_directory {
        if let Some(num) = sd.get("num_results").or_else(|| sd.get("numResults")).and_then(|v| v.as_u64()) {
            return Some(serde_json::json!({ "num_results": num }));
        }
    }
    if let Some(ref ef) = step_update.edit_file {
        if ef.get("diff_block").or_else(|| ef.get("diffBlock")).is_some() {
            let text = step_update.text.clone().unwrap_or_default();
            return Some(serde_json::json!({ "summary": text }));
        }
    }
    if let Some(ref gi) = step_update.generate_image {
        if let Some(img_name) = gi.get("image_name").or_else(|| gi.get("imageName")).and_then(|v| v.as_str()) {
            return Some(serde_json::json!({ "image_name": img_name }));
        }
    }
    None
}

fn send_tool_response(
    input_tx: &mpsc::UnboundedSender<InputEvent>,
    tc_id: &str,
    response_val: serde_json::Value,
) {
    let response_json = serde_json::to_string(&response_val).unwrap_or_default();
    let event = InputEvent {
        user_input: None,
        complex_user_input: None,
        tool_confirmation: None,
        tool_response: Some(crate::types::ToolResponse {
            id: tc_id.to_string(),
            response_json,
        }),
        question_response: None,
        halt_request: None,
        automated_trigger: None,
    };
    let _ = input_tx.send(event);
}
