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

use antigravity_sdk::{Agent, GemmaConfig, IntoContent, LocalConnectionStrategy};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize log levels from environment variables if present
    env_logger::init();

    // Configure the Gemma Config with local server endpoint and the requested model name
    let gemma = GemmaConfig {
        base_url: Some("http://host.containers.internal:11434".to_string()),
        model_name: Some("model".to_string()),
    };

    println!("Gemma Config: {:?}", gemma);

    // Configure the agent connection strategy using local gemma configuration
    let config = LocalConnectionStrategy::default().gemma_config(gemma);

    println!("Starting agent session with Gemma...");
    let my_agent = Agent::start(config).await?;

    let prompt = "Explain in one short sentence what the theory of relativity is.";
    println!("  User: {}", prompt);

    let response = my_agent.chat(Some(prompt.into_content())).await?;

    // Await the full aggregated text response.
    let response_text = response.text().await;
    println!("  Agent: {}", response_text);

    my_agent.stop().await;
    Ok(())
}
