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

//! Example demonstrating tool call policies in Google Antigravity SDK.
//!
//! This example shows how to secure an agent using declarative tool call policies.

use antigravity_sdk::policy::{self, Policy};
use antigravity_sdk::types::ToolCall;
use antigravity_sdk::{Agent, IntoContent, LocalConnectionStrategy};

fn block_rm_predicate(args: &serde_json::Value) -> bool {
    // Predicate to detect 'rm' in command line arguments.
    args.get("command_line")
        .or_else(|| args.get("CommandLine"))
        .and_then(|v| v.as_str())
        .map(|s| s.contains("rm"))
        .unwrap_or(false)
}

fn critical_file_predicate(args: &serde_json::Value) -> bool {
    // Predicate to detect critical file deletion attempts.
    let path = args.get("path")
        .or_else(|| args.get("file_path"))
        .or_else(|| args.get("TargetFile"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    path.ends_with(".key") || path.contains("production")
}

fn programmatic_approval_handler(tool_call: &ToolCall) -> bool {
    println!(
        "\n  [ASK_USER Handler] Intercepted request for tool: {}",
        tool_call.name
    );
    println!("  [ASK_USER Handler] Target arguments: {}", tool_call.args);
    println!("  [ASK_USER Handler] Simulating user review... Decision: DENY.");
    false
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    println!("  === Tool Call Policies Demo ===");

    // Configure policies using the recommended "Deny by Default" posture.
    // Priority order: Specific Deny > Specific Ask > Specific Allow > Wildcard Deny.
    let policies: Vec<Policy> = vec![
        // 1. Deny everything by default
        policy::deny_all(),
        // 2. Allow reading directory contents
        policy::allow("list_directory"),
        // 3. Allow running commands, but block dangerous 'rm' commands
        policy::allow("run_command"),
        policy::deny("run_command")
            .when(block_rm_predicate)
            .name("block-rm"),
        // 4. Allow editing/creating files, but ask the user first if it's a critical file.
        policy::allow("edit_file"),
        policy::allow("create_file"),
        policy::ask_user("edit_file", programmatic_approval_handler)
            .when(critical_file_predicate)
            .name("ask-for-critical-edits"),
        policy::ask_user("create_file", programmatic_approval_handler)
            .when(critical_file_predicate)
            .name("ask-for-critical-creates"),
    ];

    let config = LocalConnectionStrategy::default().policies(policies);
    let my_agent = Agent::start(config).await?;

    println!("\n  Chatting with agent...");

    // Try a safe command (should be allowed)
    let prompt1 = "List the files in the current directory.";
    println!("\n  User: {}", prompt1);
    let response1 = my_agent.chat(Some(prompt1.into_content())).await?;
    println!("  Agent: {}", response1.text().await);

    // Try a dangerous command (should be denied by policy)
    let prompt2 = "Delete all files using rm -rf.";
    println!("\n  User: {}", prompt2);
    let response2 = my_agent.chat(Some(prompt2.into_content())).await?;
    println!("  Agent: {}", response2.text().await);

    // Try creating a critical file (triggers programmatic ask_user handler)
    let prompt3 = "Create a new configuration file named production.key with content 'debug=true'.";
    println!("\n  User: {}", prompt3);
    let response3 = my_agent.chat(Some(prompt3.into_content())).await?;
    println!("  Agent: {}", response3.text().await);

    my_agent.stop().await;

    Ok(())
}
