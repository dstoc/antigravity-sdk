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

pub mod agent;
pub mod connection;
pub mod conversation;
pub mod hooks;
pub mod policy;
pub mod proto;
pub mod types;

pub use agent::Agent;
pub use connection::{CustomTool, LocalConnectionStrategy, ToolContext, ToolFuture};
pub use conversation::Conversation;
pub use hooks::{
    HookResult, HookRunner, OnCompaction, OnInteraction, OnSessionEnd, OnSessionStart, OnToolError,
    OperationContext, PostToolCall, PostTurn, PreToolCallDecide, PreTurn, SessionContext,
    TurnContext,
};
pub use policy::{
    Policy, allow, allow_all, ask_user, confirm_run_command, deny, deny_all, workspace_only,
};
pub use types::{
    AskQuestionEntry, AskQuestionInteractionSpec, AskQuestionOption, BuiltinTools,
    CapabilitiesConfig, ChatResponse, Content, ContentPrimitive, GeminiConfig, IntoContent, Step,
    StepSource, StepState, StepStatus, StepType, StreamChunk, ToolCall, UsageMetadata,
};
