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

pub mod proto;
pub mod types;
pub mod policy;
pub mod connection;
pub mod conversation;
pub mod agent;
pub mod hooks;

pub use agent::Agent;
pub use hooks::{
    HookRunner, HookResult, SessionContext, TurnContext, OperationContext,
    OnSessionStart, OnSessionEnd, PreTurn, PostTurn, PreToolCallDecide,
    PostToolCall, OnToolError, OnInteraction, OnCompaction,
};
pub use connection::{LocalConnectionStrategy, CustomTool, ToolContext, ToolFuture};
pub use conversation::Conversation;
pub use policy::{Policy, allow, deny, ask_user, allow_all, deny_all, confirm_run_command, workspace_only};
pub use types::{
    BuiltinTools, CapabilitiesConfig, Content, ContentPrimitive, GeminiConfig,
    IntoContent, Step, StepSource, StepState, StepStatus, StepType, ToolCall,
    UsageMetadata, ChatResponse, StreamChunk,
    AskQuestionOption, AskQuestionEntry, AskQuestionInteractionSpec,
};
