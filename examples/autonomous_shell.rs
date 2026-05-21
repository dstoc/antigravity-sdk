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

use antigravity_sdk::{Agent, LocalConnectionStrategy, IntoContent};
use antigravity_sdk::policy;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // allow_all() grants the agent full access to all tools, including
    // run_command (shell execution). This overrides the default
    // confirm_run_command() policy.
    let config = LocalConnectionStrategy::default()
        .policies(vec![policy::allow_all()]);

    let agent = Agent::start(config).await?;

    let prompt = "Run 'echo Hello from the shell!' and show me the output.";
    println!("  User: {}", prompt);

    let response = agent.chat(Some(prompt.into_content())).await?;
    let response_text = response.text().await;
    println!("  Agent: {}", response_text);

    agent.stop().await;
    Ok(())
}
