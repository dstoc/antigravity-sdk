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

//! Stateful conversation session wrapping a Connection.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::Mutex;
use futures_util::stream::{BoxStream, StreamExt};

use crate::types::{Step, StepSource, StepTarget, StreamChunk, UsageMetadata, Content, ChatResponse};
use crate::connection::Connection;

const DEFAULT_MAX_HISTORY_SIZE: usize = 10_000;

fn zero_usage() -> UsageMetadata {
    UsageMetadata {
        prompt_token_count: Some(0),
        cached_content_token_count: Some(0),
        candidates_token_count: Some(0),
        thoughts_token_count: Some(0),
        total_token_count: Some(0),
    }
}

fn add_usage(target: &mut UsageMetadata, source: &UsageMetadata) {
    if let Some(val) = source.prompt_token_count {
        target.prompt_token_count = Some(target.prompt_token_count.unwrap_or(0) + val);
    }
    if let Some(val) = source.cached_content_token_count {
        target.cached_content_token_count = Some(target.cached_content_token_count.unwrap_or(0) + val);
    }
    if let Some(val) = source.candidates_token_count {
        target.candidates_token_count = Some(target.candidates_token_count.unwrap_or(0) + val);
    }
    if let Some(val) = source.thoughts_token_count {
        target.thoughts_token_count = Some(target.thoughts_token_count.unwrap_or(0) + val);
    }
    if let Some(val) = source.total_token_count {
        target.total_token_count = Some(target.total_token_count.unwrap_or(0) + val);
    }
}

#[derive(Clone)]
pub struct Conversation {
    connection: Arc<Connection>,
    steps: Arc<Mutex<Vec<Step>>>,
    turn_start_indices: Arc<Mutex<Vec<usize>>>,
    compaction_indices: Arc<Mutex<Vec<usize>>>,
    max_history_size: usize,
    cumulative_usage: Arc<Mutex<UsageMetadata>>,
    turn_usage: Arc<Mutex<Option<UsageMetadata>>>,
}

impl Conversation {
    pub fn new(connection: Arc<Connection>) -> Self {
        Self {
            connection,
            steps: Arc::new(Mutex::new(Vec::new())),
            turn_start_indices: Arc::new(Mutex::new(Vec::new())),
            compaction_indices: Arc::new(Mutex::new(Vec::new())),
            max_history_size: DEFAULT_MAX_HISTORY_SIZE,
            cumulative_usage: Arc::new(Mutex::new(zero_usage())),
            turn_usage: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_max_history_size(connection: Arc<Connection>, max_history_size: usize) -> Self {
        Self {
            connection,
            steps: Arc::new(Mutex::new(Vec::new())),
            turn_start_indices: Arc::new(Mutex::new(Vec::new())),
            compaction_indices: Arc::new(Mutex::new(Vec::new())),
            max_history_size,
            cumulative_usage: Arc::new(Mutex::new(zero_usage())),
            turn_usage: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn send(&self, prompt: Option<Content>) -> Result<(), String> {
        if !self.connection.is_idle() {
            while !self.connection.is_idle() {
                if let Some(step) = self.connection.receive_steps().await {
                    self.record_step(step).await;
                } else {
                    break;
                }
            }
        }

        let mut starts = self.turn_start_indices.lock().await;
        let steps_len = self.steps.lock().await.len();
        starts.push(steps_len);

        let mut turn_usg = self.turn_usage.lock().await;
        *turn_usg = None;

        self.connection.send(prompt).await
    }

    async fn record_step(&self, step: Step) {
        let mut steps_guard = self.steps.lock().await;
        steps_guard.push(step.clone());

        if step.r#type == crate::types::StepType::Compaction {
            let mut comp_guard = self.compaction_indices.lock().await;
            comp_guard.push(steps_guard.len() - 1);
        }

        if let Some(ref usage) = step.usage_metadata {
            let mut cum_guard = self.cumulative_usage.lock().await;
            add_usage(&mut cum_guard, usage);

            let mut turn_guard = self.turn_usage.lock().await;
            if turn_guard.is_none() {
                *turn_guard = Some(zero_usage());
            }
            if let Some(ref mut tu) = *turn_guard {
                add_usage(tu, usage);
            }
        }

        if self.max_history_size > 0 && steps_guard.len() > self.max_history_size {
            let overflow = steps_guard.len() - self.max_history_size;
            *steps_guard = steps_guard[overflow..].to_vec();

            let mut starts_guard = self.turn_start_indices.lock().await;
            *starts_guard = starts_guard
                .iter()
                .filter(|&&i| i >= overflow)
                .map(|&i| i - overflow)
                .collect();

            let mut comp_guard = self.compaction_indices.lock().await;
            *comp_guard = comp_guard
                .iter()
                .filter(|&&i| i >= overflow)
                .map(|&i| i - overflow)
                .collect();
        }
    }

    pub fn receive_steps(&self) -> BoxStream<'static, Step> {
        struct StepsState {
            conn: Arc<Connection>,
            steps: Arc<Mutex<Vec<Step>>>,
            turn_start_indices: Arc<Mutex<Vec<usize>>>,
            compaction_indices: Arc<Mutex<Vec<usize>>>,
            max_history_size: usize,
            cumulative_usage: Arc<Mutex<UsageMetadata>>,
            turn_usage: Arc<Mutex<Option<UsageMetadata>>>,
        }

        let state = StepsState {
            conn: self.connection.clone(),
            steps: self.steps.clone(),
            turn_start_indices: self.turn_start_indices.clone(),
            compaction_indices: self.compaction_indices.clone(),
            max_history_size: self.max_history_size,
            cumulative_usage: self.cumulative_usage.clone(),
            turn_usage: self.turn_usage.clone(),
        };

        Box::pin(futures_util::stream::unfold(state, |state| async move {
            if let Some(step) = state.conn.receive_steps().await {
                {
                    // Record the step
                    let mut steps_guard = state.steps.lock().await;
                    steps_guard.push(step.clone());

                    if step.r#type == crate::types::StepType::Compaction {
                        let mut comp_guard = state.compaction_indices.lock().await;
                        comp_guard.push(steps_guard.len() - 1);
                    }

                    if let Some(ref usage) = step.usage_metadata {
                        let mut cum_guard = state.cumulative_usage.lock().await;
                        add_usage(&mut cum_guard, usage);

                        let mut turn_guard = state.turn_usage.lock().await;
                        if turn_guard.is_none() {
                            *turn_guard = Some(zero_usage());
                        }
                        if let Some(ref mut tu) = *turn_guard {
                            add_usage(tu, usage);
                        }
                    }

                    // Enforce max history size
                    if state.max_history_size > 0 && steps_guard.len() > state.max_history_size {
                        let overflow = steps_guard.len() - state.max_history_size;
                        *steps_guard = steps_guard[overflow..].to_vec();

                        let mut starts_guard = state.turn_start_indices.lock().await;
                        *starts_guard = starts_guard
                            .iter()
                            .filter(|&&i| i >= overflow)
                            .map(|&i| i - overflow)
                            .collect();

                        let mut comp_guard = state.compaction_indices.lock().await;
                        *comp_guard = comp_guard
                            .iter()
                            .filter(|&&i| i >= overflow)
                            .map(|&i| i - overflow)
                            .collect();
                    }
                }

                Some((step, state))
            } else {
                None
            }
        }))
    }

    pub fn receive_chunks(&self) -> BoxStream<'static, StreamChunk> {
        let steps_stream = self.receive_steps();
        let seen_tool_ids = HashSet::new();
        let pending_chunks = VecDeque::new();

        struct UnfoldState {
            stream: BoxStream<'static, Step>,
            seen_tool_ids: HashSet<String>,
            pending_chunks: VecDeque<StreamChunk>,
        }

        let state = UnfoldState {
            stream: steps_stream,
            seen_tool_ids,
            pending_chunks,
        };

        Box::pin(futures_util::stream::unfold(state, |mut state| async move {
            if let Some(chunk) = state.pending_chunks.pop_front() {
                return Some((chunk, state));
            }

            while let Some(step) = state.stream.next().await {
                let is_model = step.source == StepSource::Model;
                let is_target_user = step.target == StepTarget::User;

                if is_model && is_target_user {
                    if !step.thinking_delta.is_empty() {
                        state.pending_chunks.push_back(StreamChunk::Thought {
                            step_index: step.step_index,
                            text: step.thinking_delta.clone(),
                        });
                    }
                    if !step.content_delta.is_empty() {
                        state.pending_chunks.push_back(StreamChunk::Text {
                            step_index: step.step_index,
                            text: step.content_delta.clone(),
                        });
                    }
                }

                for call in step.tool_calls {
                    if let Some(ref id) = call.id {
                        if !state.seen_tool_ids.contains(id) {
                            state.seen_tool_ids.insert(id.clone());
                            state.pending_chunks.push_back(StreamChunk::ToolCall(call));
                        }
                    } else {
                        state.pending_chunks.push_back(StreamChunk::ToolCall(call));
                    }
                }

                if let Some(chunk) = state.pending_chunks.pop_front() {
                    return Some((chunk, state));
                }
            }
            None
        }))
    }

    pub async fn chat(&self, prompt: Option<Content>) -> Result<ChatResponse, String> {
        self.send(prompt).await?;
        let last_turn_usg = self.turn_usage.lock().await.clone();
        Ok(ChatResponse::new(self.receive_chunks(), last_turn_usg, Some(self.steps.clone())))
    }

    pub async fn send_trigger_notification(&self, message: &str) -> Result<(), String> {
        self.connection.send_trigger_notification(message).await
    }

    pub async fn history(&self) -> Vec<Step> {
        self.steps.lock().await.clone()
    }

    pub async fn last_response(&self) -> String {
        let guard = self.steps.lock().await;
        for step in guard.iter().rev() {
            if step.is_complete_response.unwrap_or(false) {
                return step.content.clone();
            }
        }
        String::new()
    }

    pub async fn turn_count(&self) -> usize {
        self.turn_start_indices.lock().await.len()
    }

    pub async fn compaction_indices(&self) -> Vec<usize> {
        self.compaction_indices.lock().await.clone()
    }

    pub async fn clear_history(&self) {
        self.steps.lock().await.clear();
        self.turn_start_indices.lock().await.clear();
        self.compaction_indices.lock().await.clear();
        *self.cumulative_usage.lock().await = zero_usage();
        *self.turn_usage.lock().await = None;
    }

    pub fn connection(&self) -> Arc<Connection> {
        self.connection.clone()
    }

    pub fn is_idle(&self) -> bool {
        self.connection.is_idle()
    }

    pub fn conversation_id(&self) -> String {
        self.connection.conversation_id()
    }

    pub async fn total_usage(&self) -> UsageMetadata {
        self.cumulative_usage.lock().await.clone()
    }

    pub async fn last_turn_usage(&self) -> Option<UsageMetadata> {
        self.turn_usage.lock().await.clone()
    }

    pub async fn cancel(&self) -> Result<(), String> {
        self.connection.cancel().await
    }

    pub async fn wait_for_idle(&self) {
        self.connection.wait_for_idle().await;
    }

    pub async fn disconnect(&self) {
        self.connection.disconnect().await;
    }
}
