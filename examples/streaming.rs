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

//! Example demonstrating streaming responses and thoughts in Google Antigravity SDK.

use antigravity_sdk::{Agent, IntoContent, LocalConnectionStrategy};
use futures_util::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let config = LocalConnectionStrategy::default();
    let my_agent = Agent::start(config).await?;

    let prompt = "Solve this riddle: I speak without a mouth and hear without ears. I \
                  have no body, but I come alive with wind. What am I? Explain your \
                  reasoning.";
    println!("  User: {}\n", prompt);

    let response = my_agent.chat(Some(prompt.into_content())).await?;

    println!("  Agent (Streaming thoughts):");
    println!("  -------------------------------------------------------");
    let mut thoughts_stream = response.thoughts();
    while let Some(thought) = thoughts_stream.next().await {
        print!("{}", thought);
        std::io::Write::flush(&mut std::io::stdout())?;
    }
    println!("\n  -------------------------------------------------------\n");

    println!("  Agent (Streaming final answer):");
    println!("  -------------------------------------------------------");
    let mut text_stream = response.text_stream();
    while let Some(token) = text_stream.next().await {
        print!("{}", token);
        std::io::Write::flush(&mut std::io::stdout())?;
    }
    println!("\n  -------------------------------------------------------\n");

    my_agent.stop().await;

    Ok(())
}
