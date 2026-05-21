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

//! Example demonstrating subagents in Google Antigravity SDK.
//!
//! This example shows how an agent can spawn a subagent to delegate a specific
//! task, in this case, researching the examples directory to generate a lesson
//! plan.

use std::sync::{Arc, Mutex};
use antigravity_sdk::hooks::HookResult;
use antigravity_sdk::types::{CapabilitiesConfig, ToolCall, ToolResult};
use antigravity_sdk::{Agent, IntoContent, LocalConnectionStrategy};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Track active subagents across hooks using a thread-safe Mutex
    let subagent_active = Arc::new(Mutex::new(false));

    // Configure the agent with subagent capabilities enabled, and hooks for visibility.
    let capabilities = CapabilitiesConfig {
        enable_subagents: true,
        ..Default::default()
    };

    let sa_active1 = subagent_active.clone();
    let sa_active2 = subagent_active.clone();

    let config = LocalConnectionStrategy::default()
        .capabilities(capabilities)
        .register_pre_tool_call_decide(move |_context, tool_call: ToolCall| {
            let sa_active = sa_active1.clone();
            async move {
                if tool_call.name == "start_subagent" {
                    *sa_active.lock().unwrap() = true;
                    println!("\n  --- 🤖 [Hook] Spawning Subagent ---");
                    println!("  Arguments: {}\n", tool_call.args);
                } else {
                    let active = *sa_active.lock().unwrap();
                    let indent = if active { "    " } else { "  " };
                    println!(
                        "{}- [Start]: {} (ID: {})",
                        indent,
                        tool_call.name,
                        tool_call.id.as_deref().unwrap_or("")
                    );
                }
                Ok(HookResult::allow())
            }
        })
        .register_post_tool_call(move |_context, tool_result: ToolResult| {
            let sa_active = sa_active2.clone();
            async move {
                if tool_result.name == "start_subagent" {
                    *sa_active.lock().unwrap() = false;
                    println!("\n  --- 🤖 [Hook] Subagent Finished ---");
                    println!("  Result: {}\n", tool_result.result);
                } else {
                    let active = *sa_active.lock().unwrap();
                    let indent = if active { "    " } else { "  " };
                    println!(
                        "{}- [Done]: {} (ID: {}) ✅",
                        indent,
                        tool_result.name,
                        tool_result.id.as_deref().unwrap_or("")
                    );
                }
                Ok(())
            }
        });

    let my_agent = Agent::start(config).await?;

    let prompt = "Use a subagent to research the Google Antigravity SDK examples in the \
                  parent directory. Delegate the task of listing and reading the files to the \
                  subagent, and then generate a lesson plan for me to learn more based \
                  on its findings.";

    println!("  User: {}", prompt);
    let response = my_agent.chat(Some(prompt.into_content())).await?;

    // Await the full aggregated text response. This includes both the
    // subagent's output and the main agent's regular response text.
    let response_text = response.text().await;
    println!("\n  Agent:\n{}", response_text);

    my_agent.stop().await;

    Ok(())
}
