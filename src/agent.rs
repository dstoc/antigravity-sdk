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

//! High-level Agent orchestrator matching Layer 1 API.

use crate::conversation::Conversation;
use crate::connection::LocalConnectionStrategy;
use crate::types::{Content, ChatResponse};

pub struct Agent {
    conversation: Conversation,
}

impl Agent {
    /// Starts a new agent session using the provided local connection strategy.
    pub async fn start(strategy: LocalConnectionStrategy) -> Result<Self, String> {
        log::info!("Starting Agent session...");
        let connection = strategy.connect().await?;
        let conversation = Conversation::new(connection);
        Ok(Self { conversation })
    }

    /// Sends a prompt to the agent and returns a ChatResponse stream.
    pub async fn chat(&self, prompt: Option<Content>) -> Result<ChatResponse, String> {
        self.conversation.chat(prompt).await
    }

    /// Returns the active Conversation session.
    pub fn conversation(&self) -> &Conversation {
        &self.conversation
    }

    /// Returns the conversation identifier assigned by the runtime.
    pub fn conversation_id(&self) -> String {
        self.conversation.conversation_id()
    }

    /// Stops the agent session and releases all resources.
    pub async fn stop(self) {
        log::info!("Stopping Agent session");
        self.conversation.disconnect().await;
    }
}
