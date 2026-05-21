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

//! Strongly-typed domain models for the Google Antigravity SDK.

use serde::{Deserialize, Deserializer, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use futures_util::stream::{BoxStream, StreamExt};

// =============================================================================
// Error Types
// =============================================================================

#[derive(Debug)]
pub enum AntigravityError {
    Connection(String),
    Validation(String),
    Other(String),
}

impl std::fmt::Display for AntigravityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AntigravityError::Connection(msg) => write!(f, "Connection Error: {}", msg),
            AntigravityError::Validation(msg) => write!(f, "Validation Error: {}", msg),
            AntigravityError::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl std::error::Error for AntigravityError {}

// =============================================================================
// Enums with Custom Deserializers (supporting both string names and integer tags)
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum StepState {
    Unspecified = 0,
    Active = 1,
    Done = 2,
    WaitingForUser = 3,
    Error = 4,
}

impl<'de> Deserialize<'de> for StepState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = StepState;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("integer or string representing StepState")
            }
            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match v {
                    1 => Ok(StepState::Active),
                    2 => Ok(StepState::Done),
                    3 => Ok(StepState::WaitingForUser),
                    4 => Ok(StepState::Error),
                    _ => Ok(StepState::Unspecified),
                }
            }
            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_i64(v as i64)
            }
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match v {
                    "STATE_ACTIVE" | "Active" | "active" => Ok(StepState::Active),
                    "STATE_DONE" | "Done" | "done" => Ok(StepState::Done),
                    "STATE_WAITING_FOR_USER" | "WaitingForUser" | "waiting_for_user" => Ok(StepState::WaitingForUser),
                    "STATE_ERROR" | "Error" | "error" => Ok(StepState::Error),
                    _ => Ok(StepState::Unspecified),
                }
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum StepSource {
    Unknown = 0,
    System = 1,
    User = 2,
    Model = 3,
}

impl<'de> Deserialize<'de> for StepSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = StepSource;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("integer or string representing StepSource")
            }
            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match v {
                    1 => Ok(StepSource::System),
                    2 => Ok(StepSource::User),
                    3 => Ok(StepSource::Model),
                    _ => Ok(StepSource::Unknown),
                }
            }
            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_i64(v as i64)
            }
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match v {
                    "SOURCE_SYSTEM" | "System" | "system" => Ok(StepSource::System),
                    "SOURCE_USER" | "User" | "user" => Ok(StepSource::User),
                    "SOURCE_MODEL" | "Model" | "model" => Ok(StepSource::Model),
                    _ => Ok(StepSource::Unknown),
                }
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum StepTarget {
    Unknown = 0,
    User = 1,
    Model = 2,
    Environment = 3,
}

impl<'de> Deserialize<'de> for StepTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = StepTarget;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("integer or string representing StepTarget")
            }
            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match v {
                    1 => Ok(StepTarget::User),
                    2 => Ok(StepTarget::Model),
                    3 => Ok(StepTarget::Environment),
                    _ => Ok(StepTarget::Unknown),
                }
            }
            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_i64(v as i64)
            }
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match v {
                    "TARGET_USER" | "User" | "user" => Ok(StepTarget::User),
                    "TARGET_MODEL" | "Model" | "model" => Ok(StepTarget::Model),
                    "TARGET_ENVIRONMENT" | "Environment" | "environment" => Ok(StepTarget::Environment),
                    _ => Ok(StepTarget::Unknown),
                }
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepType {
    #[serde(rename = "TEXT_RESPONSE")]
    TextResponse,
    #[serde(rename = "TOOL_CALL")]
    ToolCall,
    #[serde(rename = "SYSTEM_MESSAGE")]
    SystemMessage,
    #[serde(rename = "COMPACTION")]
    Compaction,
    #[serde(rename = "FINISH")]
    Finish,
    #[serde(rename = "UNKNOWN")]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepStatus {
    #[serde(rename = "ACTIVE")]
    Active,
    #[serde(rename = "DONE")]
    Done,
    #[serde(rename = "WAITING_FOR_USER")]
    WaitingForUser,
    #[serde(rename = "ERROR")]
    Error,
    #[serde(rename = "CANCELED")]
    Canceled,
    #[serde(rename = "UNKNOWN")]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BuiltinTools {
    #[serde(rename = "list_directory")]
    ListDir,
    #[serde(rename = "search_directory")]
    SearchDir,
    #[serde(rename = "find_file")]
    FindFile,
    #[serde(rename = "view_file")]
    ViewFile,
    #[serde(rename = "create_file")]
    CreateFile,
    #[serde(rename = "edit_file")]
    EditFile,
    #[serde(rename = "run_command")]
    RunCommand,
    #[serde(rename = "ask_question")]
    AskQuestion,
    #[serde(rename = "start_subagent")]
    StartSubagent,
    #[serde(rename = "generate_image")]
    GenerateImage,
    #[serde(rename = "finish")]
    Finish,
}

impl BuiltinTools {
    pub fn as_str(&self) -> &'static str {
        match self {
            BuiltinTools::ListDir => "list_directory",
            BuiltinTools::SearchDir => "search_directory",
            BuiltinTools::FindFile => "find_file",
            BuiltinTools::ViewFile => "view_file",
            BuiltinTools::CreateFile => "create_file",
            BuiltinTools::EditFile => "edit_file",
            BuiltinTools::RunCommand => "run_command",
            BuiltinTools::AskQuestion => "ask_question",
            BuiltinTools::StartSubagent => "start_subagent",
            BuiltinTools::GenerateImage => "generate_image",
            BuiltinTools::Finish => "finish",
        }
    }

    pub fn read_only() -> Vec<Self> {
        vec![
            BuiltinTools::ListDir,
            BuiltinTools::SearchDir,
            BuiltinTools::FindFile,
            BuiltinTools::ViewFile,
            BuiltinTools::Finish,
        ]
    }

    pub fn file_tools() -> Vec<Self> {
        vec![
            BuiltinTools::ViewFile,
            BuiltinTools::CreateFile,
            BuiltinTools::EditFile,
        ]
    }

    pub fn all_tools() -> Vec<Self> {
        vec![
            BuiltinTools::ListDir,
            BuiltinTools::SearchDir,
            BuiltinTools::FindFile,
            BuiltinTools::ViewFile,
            BuiltinTools::CreateFile,
            BuiltinTools::EditFile,
            BuiltinTools::RunCommand,
            BuiltinTools::AskQuestion,
            BuiltinTools::StartSubagent,
            BuiltinTools::GenerateImage,
            BuiltinTools::Finish,
        ]
    }
}

// =============================================================================
// Configuration Types
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThinkingLevel {
    #[serde(rename = "minimal")]
    Minimal,
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
}

impl ThinkingLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThinkingLevel::Minimal => "minimal",
            ThinkingLevel::Low => "low",
            ThinkingLevel::Medium => "medium",
            ThinkingLevel::High => "high",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationConfig {
    #[serde(rename = "thinkingLevel", alias = "thinking_level", skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<GenerationConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub default: ModelEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiConfig {
    pub models: ModelConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireGeminiConfig {
    #[serde(rename = "apiKey", alias = "api_key", skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(rename = "baseUrl", alias = "base_url", skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(rename = "modelName", alias = "model_name", skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(rename = "thinkingLevel", alias = "thinking_level", skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
    #[serde(rename = "enableUrlContext", alias = "enable_url_context", skip_serializing_if = "Option::is_none")]
    pub enable_url_context: Option<bool>,
    #[serde(rename = "enableGoogleSearch", alias = "enable_google_search", skip_serializing_if = "Option::is_none")]
    pub enable_google_search: Option<bool>,
}

impl GeminiConfig {
    pub fn to_wire(&self) -> WireGeminiConfig {
        let effective_api_key = self.models.default.api_key.clone().or_else(|| self.api_key.clone());
        let thinking_level = self.models.default.generation.as_ref()
            .and_then(|g| g.thinking_level.map(|tl| tl.as_str().to_string()));
        WireGeminiConfig {
            api_key: effective_api_key,
            base_url: None,
            model_name: Some(self.models.default.name.clone()),
            thinking_level,
            enable_url_context: None,
            enable_google_search: None,
        }
    }
}

impl Default for GeminiConfig {
    fn default() -> Self {
        Self {
            models: ModelConfig {
                default: ModelEntry {
                    name: "gemini-2.5-flash".to_string(),
                    api_key: None,
                    generation: None,
                },
            },
            api_key: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInstructionSection {
    pub title: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomSystemInstructionPart {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomSystemInstructions {
    pub part: Vec<CustomSystemInstructionPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendedSystemInstructions {
    #[serde(rename = "customIdentity", alias = "custom_identity", skip_serializing_if = "Option::is_none")]
    pub custom_identity: Option<String>,
    #[serde(rename = "appendedSections", alias = "appended_sections")]
    pub appended_sections: Vec<SystemInstructionSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInstructions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom: Option<CustomSystemInstructions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub appended: Option<AppendedSystemInstructions>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemWorkspace {
    pub directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    #[serde(rename = "filesystemWorkspace", alias = "filesystem_workspace")]
    pub filesystem_workspace: FilesystemWorkspace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    #[serde(rename = "parametersJsonSchema", alias = "parameters_json_schema")]
    pub parameters_json_schema: String,
    #[serde(rename = "responseJsonSchema", alias = "response_json_schema", skip_serializing_if = "Option::is_none")]
    pub response_json_schema: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindToolConfig { pub enabled: bool }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunCommandToolConfig { pub enabled: bool }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentsConfig { pub enabled: bool }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserQuestionsConfig { pub enabled: bool }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEditToolConfig { pub enabled: bool }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewFileToolConfig { pub enabled: bool }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteToFileToolConfig { pub enabled: bool }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepSearchToolConfig { pub enabled: bool }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDirToolConfig { pub enabled: bool }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateImageToolConfig {
    pub enabled: bool,
    #[serde(rename = "modelName", alias = "model_name", skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessSideTools {
    pub find: FindToolConfig,
    #[serde(rename = "runCommand", alias = "run_command")]
    pub run_command: RunCommandToolConfig,
    pub subagents: SubagentsConfig,
    #[serde(rename = "userQuestions", alias = "user_questions")]
    pub user_questions: UserQuestionsConfig,
    #[serde(rename = "fileEdit", alias = "file_edit")]
    pub file_edit: FileEditToolConfig,
    #[serde(rename = "viewFile", alias = "view_file")]
    pub view_file: ViewFileToolConfig,
    #[serde(rename = "writeToFile", alias = "write_to_file")]
    pub write_to_file: WriteToFileToolConfig,
    #[serde(rename = "grepSearch", alias = "grep_search")]
    pub grep_search: GrepSearchToolConfig,
    #[serde(rename = "listDir", alias = "list_dir")]
    pub list_dir: ListDirToolConfig,
    #[serde(rename = "generateImage", alias = "generate_image")]
    pub generate_image: GenerateImageToolConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessConfig {
    pub tools: Vec<Tool>,
    #[serde(rename = "systemInstructions", alias = "system_instructions", skip_serializing_if = "Option::is_none")]
    pub system_instructions: Option<SystemInstructions>,
    #[serde(rename = "cascadeId", alias = "cascade_id")]
    pub cascade_id: String,
    #[serde(rename = "geminiConfig", alias = "gemini_config", skip_serializing_if = "Option::is_none")]
    pub gemini_config: Option<WireGeminiConfig>,
    pub workspaces: Vec<Workspace>,
    #[serde(rename = "skillsPaths", alias = "skills_paths")]
    pub skills_paths: Vec<String>,
    #[serde(rename = "harnessSideTools", alias = "harness_side_tools")]
    pub harness_side_tools: HarnessSideTools,
    #[serde(rename = "compactionThreshold", alias = "compaction_threshold")]
    pub compaction_threshold: u32,
    #[serde(rename = "finishToolSchemaJson", alias = "finish_tool_schema_json")]
    pub finish_tool_schema_json: String,
    #[serde(rename = "appDataDir", alias = "app_data_dir")]
    pub app_data_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitiesConfig {
    pub enabled_tools: Option<Vec<BuiltinTools>>,
    pub disabled_tools: Option<Vec<BuiltinTools>>,
    pub enable_subagents: bool,
    pub image_model: String,
    pub compaction_threshold: Option<u32>,
    pub finish_tool_schema_json: Option<String>,
}

impl Default for CapabilitiesConfig {
    fn default() -> Self {
        Self {
            enabled_tools: None,
            disabled_tools: None,
            enable_subagents: true,
            image_model: "imagen-3.0-generate-002".to_string(),
            compaction_threshold: None,
            finish_tool_schema_json: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
}

// =============================================================================
// Live Execution Events and Steps
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub args: serde_json::Value,
    #[serde(rename = "argumentsJson", alias = "arguments_json", skip_serializing_if = "Option::is_none")]
    pub arguments_json: Option<String>,
    #[serde(rename = "canonicalPath", alias = "canonical_path", skip_serializing_if = "Option::is_none")]
    pub canonical_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub id: Option<String>,
    pub name: String,
    pub result: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn deserialize_u64_or_str<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;
    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = Option<u64>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("integer or string representing integer")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_some<D2>(self, deserializer: D2) -> Result<Self::Value, D2::Error>
        where
            D2: serde::Deserializer<'de>,
        {
            deserializer.deserialize_any(self)
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(v))
        }

        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            if v >= 0 {
                Ok(Some(v as u64))
            } else {
                Err(serde::de::Error::custom("negative sequence number"))
            }
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            v.parse::<u64>().map(Some).map_err(serde::de::Error::custom)
        }
    }

    deserializer.deserialize_option(Visitor)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageMetadata {
    #[serde(rename = "promptTokenCount", alias = "prompt_token_count", deserialize_with = "deserialize_u64_or_str", default)]
    pub prompt_token_count: Option<u64>,
    #[serde(rename = "cachedContentTokenCount", alias = "cached_content_token_count", deserialize_with = "deserialize_u64_or_str", default)]
    pub cached_content_token_count: Option<u64>,
    #[serde(rename = "candidatesTokenCount", alias = "candidates_token_count", deserialize_with = "deserialize_u64_or_str", default)]
    pub candidates_token_count: Option<u64>,
    #[serde(rename = "thoughtsTokenCount", alias = "thoughts_token_count", deserialize_with = "deserialize_u64_or_str", default)]
    pub thoughts_token_count: Option<u64>,
    #[serde(rename = "totalTokenCount", alias = "total_token_count", deserialize_with = "deserialize_u64_or_str", default)]
    pub total_token_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub step_index: u32,
    pub r#type: StepType,
    pub source: StepSource,
    pub target: StepTarget,
    pub status: StepStatus,
    pub content: String,
    pub thinking: String,
    pub content_delta: String,
    pub thinking_delta: String,
    pub tool_calls: Vec<ToolCall>,
    pub error: String,
    pub is_complete_response: Option<bool>,
    pub structured_output: Option<serde_json::Value>,
    pub usage_metadata: Option<UsageMetadata>,
}

impl Default for Step {
    fn default() -> Self {
        Self {
            id: String::new(),
            step_index: 0,
            r#type: StepType::Unknown,
            source: StepSource::Unknown,
            target: StepTarget::Unknown,
            status: StepStatus::Unknown,
            content: String::new(),
            thinking: String::new(),
            content_delta: String::new(),
            thinking_delta: String::new(),
            tool_calls: Vec::new(),
            error: String::new(),
            is_complete_response: None,
            structured_output: None,
            usage_metadata: None,
        }
    }
}

// =============================================================================
// WebSocket Event Serialization / Deserialization
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeConversationEvent {
    pub config: HarnessConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputEvent {
    #[serde(rename = "seqNum", alias = "seq_num", deserialize_with = "deserialize_u64_or_str", default)]
    pub seq_num: Option<u64>,
    #[serde(rename = "timestampMicros", alias = "timestamp_micros", deserialize_with = "deserialize_u64_or_str", default)]
    pub timestamp_micros: Option<u64>,
    #[serde(rename = "stepUpdate", alias = "step_update")]
    pub step_update: Option<StepUpdate>,
    #[serde(rename = "trajectoryStateUpdate", alias = "trajectory_state_update")]
    pub trajectory_state_update: Option<TrajectoryStateUpdate>,
    #[serde(rename = "toolCall", alias = "tool_call")]
    pub tool_call: Option<ToolCall>,
    #[serde(rename = "usageMetadata", alias = "usage_metadata")]
    pub usage_metadata: Option<UsageMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepUpdate {
    #[serde(rename = "cascadeId", alias = "cascade_id")]
    pub cascade_id: Option<String>,
    #[serde(rename = "trajectoryId", alias = "trajectory_id")]
    pub trajectory_id: Option<String>,
    #[serde(rename = "stepIndex", alias = "step_index")]
    pub step_index: u32,
    pub state: StepState,
    pub source: StepSource,
    pub target: StepTarget,
    #[serde(rename = "errorMessage", alias = "error_message")]
    pub error_message: Option<String>,
    pub thinking: Option<String>,
    #[serde(rename = "textDelta", alias = "text_delta")]
    pub text_delta: Option<String>,
    #[serde(rename = "thinkingDelta", alias = "thinking_delta")]
    pub thinking_delta: Option<String>,
    pub text: Option<String>,
    #[serde(rename = "listDirectory", alias = "list_directory")]
    pub list_directory: Option<serde_json::Value>,
    #[serde(rename = "findFile", alias = "find_file")]
    pub find_file: Option<serde_json::Value>,
    #[serde(rename = "searchDirectory", alias = "search_directory")]
    pub search_directory: Option<serde_json::Value>,
    #[serde(rename = "viewFile", alias = "view_file")]
    pub view_file: Option<serde_json::Value>,
    #[serde(rename = "createFile", alias = "create_file")]
    pub create_file: Option<serde_json::Value>,
    #[serde(rename = "editFile", alias = "edit_file")]
    pub edit_file: Option<serde_json::Value>,
    #[serde(rename = "runCommand", alias = "run_command")]
    pub run_command: Option<serde_json::Value>,
    pub compaction: Option<serde_json::Value>,
    #[serde(rename = "invokeSubagent", alias = "invoke_subagent")]
    pub invoke_subagent: Option<serde_json::Value>,
    #[serde(rename = "generateImage", alias = "generate_image")]
    pub generate_image: Option<serde_json::Value>,
    pub finish: Option<serde_json::Value>,
    pub error: Option<ActionError>,
    #[serde(rename = "requestText", alias = "request_text")]
    pub request_text: Option<String>,
    #[serde(rename = "toolConfirmationRequest", alias = "tool_confirmation_request")]
    pub tool_confirmation_request: Option<serde_json::Value>,
    #[serde(rename = "questionsRequest", alias = "questions_request")]
    pub questions_request: Option<UserQuestionsRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionError {
    #[serde(rename = "errorMessage", alias = "error_message")]
    pub error_message: String,
    #[serde(rename = "httpCode", alias = "http_code")]
    pub http_code: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserQuestionsRequest {
    pub questions: Vec<UserQuestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskQuestionOption {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskQuestionEntry {
    pub question: String,
    pub options: Vec<AskQuestionOption>,
    #[serde(rename = "isMultiSelect", alias = "is_multi_select", default)]
    pub is_multi_select: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskQuestionInteractionSpec {
    pub questions: Vec<AskQuestionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserQuestion {
    #[serde(rename = "multipleChoice", alias = "multiple_choice")]
    pub multiple_choice: MultipleChoice,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultipleChoice {
    pub question: String,
    pub choices: Vec<String>,
    #[serde(rename = "isMultiSelect", alias = "is_multi_select")]
    pub is_multi_select: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryStateUpdate {
    #[serde(rename = "trajectoryId", alias = "trajectory_id")]
    pub trajectory_id: String,
    pub state: TrajectoryState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TrajectoryState {
    Unspecified = 0,
    Running = 1,
    Idle = 2,
}

impl<'de> Deserialize<'de> for TrajectoryState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = TrajectoryState;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("integer or string representing TrajectoryState")
            }
            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match v {
                    1 => Ok(TrajectoryState::Running),
                    2 => Ok(TrajectoryState::Idle),
                    _ => Ok(TrajectoryState::Unspecified),
                }
            }
            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_i64(v as i64)
            }
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match v {
                    "STATE_RUNNING" | "Running" | "running" => Ok(TrajectoryState::Running),
                    "STATE_IDLE" | "Idle" | "idle" => Ok(TrajectoryState::Idle),
                    _ => Ok(TrajectoryState::Unspecified),
                }
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputEvent {
    #[serde(rename = "userInput", alias = "user_input", skip_serializing_if = "Option::is_none")]
    pub user_input: Option<String>,
    #[serde(rename = "complexUserInput", alias = "complex_user_input", skip_serializing_if = "Option::is_none")]
    pub complex_user_input: Option<UserInput>,
    #[serde(rename = "toolConfirmation", alias = "tool_confirmation", skip_serializing_if = "Option::is_none")]
    pub tool_confirmation: Option<ToolConfirmation>,
    #[serde(rename = "toolResponse", alias = "tool_response", skip_serializing_if = "Option::is_none")]
    pub tool_response: Option<ToolResponse>,
    #[serde(rename = "questionResponse", alias = "question_response", skip_serializing_if = "Option::is_none")]
    pub question_response: Option<UserQuestionsResponse>,
    #[serde(rename = "haltRequest", alias = "halt_request", skip_serializing_if = "Option::is_none")]
    pub halt_request: Option<bool>,
    #[serde(rename = "automatedTrigger", alias = "automated_trigger", skip_serializing_if = "Option::is_none")]
    pub automated_trigger: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInput {
    pub parts: Vec<UserInputPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInputPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<MediaInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInput {
    #[serde(rename = "mimeType", alias = "mime_type")]
    pub mime_type: String,
    pub data: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfirmation {
    #[serde(rename = "trajectoryId", alias = "trajectory_id")]
    pub trajectory_id: String,
    #[serde(rename = "stepIndex", alias = "step_index")]
    pub step_index: u32,
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResponse {
    pub id: String,
    #[serde(rename = "responseJson", alias = "response_json")]
    pub response_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserQuestionsResponse {
    #[serde(rename = "trajectoryId", alias = "trajectory_id")]
    pub trajectory_id: String,
    #[serde(rename = "stepIndex", alias = "step_index")]
    pub step_index: u32,
    pub response: QuestionsResponseInner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionsResponseInner {
    pub answers: Vec<UserQuestionAnswer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserQuestionAnswer {
    pub unanswered: bool,
    #[serde(rename = "multipleChoiceAnswer", alias = "multiple_choice_answer", skip_serializing_if = "Option::is_none")]
    pub multiple_choice_answer: Option<MultipleChoiceAnswer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultipleChoiceAnswer {
    #[serde(rename = "selectedChoiceIndices", alias = "selected_choice_indices")]
    pub selected_choice_indices: Vec<u32>,
    #[serde(rename = "freeformResponse", alias = "freeform_response", skip_serializing_if = "Option::is_none")]
    pub freeform_response: Option<String>,
}

// =============================================================================
// Input Content Primitives for User Prompts
// =============================================================================

#[derive(Debug, Clone)]
pub struct Media {
    pub mime_type: String,
    pub data: Vec<u8>,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ContentPrimitive {
    Text(String),
    Media(Media),
}

pub type Content = Vec<ContentPrimitive>;

pub trait IntoContent {
    fn into_content(self) -> Content;
}

impl IntoContent for String {
    fn into_content(self) -> Content {
        vec![ContentPrimitive::Text(self)]
    }
}

impl IntoContent for &str {
    fn into_content(self) -> Content {
        vec![ContentPrimitive::Text(self.to_string())]
    }
}

impl IntoContent for Content {
    fn into_content(self) -> Content {
        self
    }
}

// =============================================================================
// Streaming and Response Types
// =============================================================================

#[derive(Debug, Clone)]
pub enum StreamChunk {
    Thought { step_index: u32, text: String },
    Text { step_index: u32, text: String },
    ToolCall(ToolCall),
}

struct ChatResponseState {
    stream: BoxStream<'static, StreamChunk>,
    buffered: Vec<StreamChunk>,
    is_done: bool,
    last_turn_usage: Option<UsageMetadata>,
}

#[derive(Clone)]
pub struct ChatResponse {
    state: Arc<Mutex<ChatResponseState>>,
}

impl ChatResponse {
    pub fn new(
        stream: BoxStream<'static, StreamChunk>,
        last_turn_usage: Option<UsageMetadata>,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(ChatResponseState {
                stream,
                buffered: Vec::new(),
                is_done: false,
                last_turn_usage,
            })),
        }
    }

    pub async fn text(&self) -> String {
        let chunks = self.resolve().await;
        let mut out = String::new();
        for chunk in chunks {
            if let StreamChunk::Text { text, .. } = chunk {
                out.push_str(&text);
            }
        }
        out
    }

    pub async fn thinking(&self) -> String {
        let chunks = self.resolve().await;
        let mut out = String::new();
        for chunk in chunks {
            if let StreamChunk::Thought { text, .. } = chunk {
                out.push_str(&text);
            }
        }
        out
    }

    pub async fn resolve(&self) -> Vec<StreamChunk> {
        let mut state = self.state.lock().await;
        while !state.is_done {
            if let Some(chunk) = state.stream.next().await {
                state.buffered.push(chunk);
            } else {
                state.is_done = true;
            }
        }
        state.buffered.clone()
    }

    pub fn chunks(&self) -> BoxStream<'static, StreamChunk> {
        let state = self.state.clone();
        let pos = 0;
        Box::pin(futures_util::stream::unfold((state, pos), |(state, pos)| async move {
            let mut s = state.lock().await;
            if pos < s.buffered.len() {
                let chunk = s.buffered[pos].clone();
                drop(s);
                Some((chunk, (state, pos + 1)))
            } else if s.is_done {
                None
            } else {
                if let Some(chunk) = s.stream.next().await {
                    s.buffered.push(chunk.clone());
                    drop(s);
                    Some((chunk, (state, pos + 1)))
                } else {
                    s.is_done = true;
                    None
                }
            }
        }))
    }

    pub async fn usage_metadata(&self) -> Option<UsageMetadata> {
        let state = self.state.lock().await;
        state.last_turn_usage.clone()
    }
}
