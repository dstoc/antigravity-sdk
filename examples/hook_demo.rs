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

//! Example demonstrating the lifecycle hooks framework in the Antigravity Rust SDK.

use antigravity_sdk::hooks::{HookResult, SessionContext, TurnContext};
use antigravity_sdk::{Agent, Content, ContentPrimitive, IntoContent, LocalConnectionStrategy};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize log levels from environment variables if present
    env_logger::init();

    // Track statistics across hooks
    let turn_counter = Arc::new(AtomicUsize::new(0));

    // Configure the agent strategy and register lifecycle hooks
    let turn_counter_clone = turn_counter.clone();
    let config = LocalConnectionStrategy::default()
        // Hook 1: On Session Start
        .register_on_session_start(|_context: SessionContext| async move {
            println!("[Hook] OnSessionStart: Agent session is beginning!");
            Ok(())
        })
        // Hook 2: Pre Turn
        .register_pre_turn(move |context: TurnContext, prompt: Content| {
            let tc = turn_counter_clone.clone();
            async move {
                let current_turn = tc.fetch_add(1, Ordering::SeqCst) + 1;
                println!("[Hook] PreTurn: Starting turn #{}!", current_turn);

                // Store metadata in the TurnContext (which is isolated per turn)
                context.set("turn_number", serde_json::json!(current_turn));

                for primitive in &prompt {
                    if let ContentPrimitive::Text(text) = primitive {
                        println!("[Hook] User prompt: '{}'", text);
                    }
                }
                Ok(HookResult::allow())
            }
        })
        // Hook 3: Post Turn
        .register_post_turn(|context: TurnContext, response: String| async move {
            let turn_num = context
                .get("turn_number")
                .unwrap_or_else(|| serde_json::json!(0));
            println!("[Hook] PostTurn for turn #{} completed!", turn_num);
            println!("[Hook] Response length: {} characters", response.len());
            Ok(())
        });

    println!("Starting agent session with hooks registered...");
    let my_agent = Agent::start(config).await?;

    let prompt = "Say 'Hello World from Hooks!'";
    println!("\n  User: {}", prompt);

    let response = my_agent.chat(Some(prompt.into_content())).await?;
    let response_text = response.text().await;
    println!("  Agent: {}\n", response_text);

    my_agent.stop().await;
    Ok(())
}
