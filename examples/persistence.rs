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

//! Example demonstrating stateful session resumption in Google Antigravity SDK.
//!
//! This example shows how to persist conversation state across process restarts
//! using a conversation ID and a storage directory.

use tempfile::tempdir;
use antigravity_sdk::{Agent, IntoContent, LocalConnectionStrategy};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let temp_dir = tempdir()?;
    let save_dir = temp_dir.path().to_string_lossy().into_owned();
    println!("  Save directory: {}", save_dir);

    println!("\n  === Session 1: establishing context ===");

    // Specify `save_dir` to ensure conversation history and artifacts are
    // persisted to disk.
    let config1 = LocalConnectionStrategy::new(save_dir.clone());
    let my_agent1 = Agent::start(config1).await?;

    let prompt1 = "Remember this: my favorite color is blue.";
    println!("  User: {}", prompt1);
    let response1 = my_agent1.chat(Some(prompt1.into_content())).await?;
    println!("  Agent: {}", response1.text().await);

    // Read back the conversation_id assigned by the runtime.
    let conversation_id = my_agent1.conversation_id();
    println!("  Assigned conversation ID: {}", conversation_id);
    my_agent1.stop().await;
    println!("  Session 1 ended.\n");

    println!("  === Session 2: resuming and verifying recall ===");
    // By providing the exact same `save_dir` and the prior `conversation_id`,
    // the new agent instance automatically restores the previous conversation
    // history and context.
    let config2 = LocalConnectionStrategy::new(save_dir)
        .conversation_id(conversation_id);
    let my_agent2 = Agent::start(config2).await?;

    let prompt2 = "What is my favorite color?";
    println!("  User: {}", prompt2);
    let response2 = my_agent2.chat(Some(prompt2.into_content())).await?;
    println!("  Agent: {}", response2.text().await);
    my_agent2.stop().await;
    println!("  Session 2 ended.");

    Ok(())
}
