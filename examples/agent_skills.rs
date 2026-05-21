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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let curr = std::env::current_dir()?;
    let skill_path = curr.join("3p/antigravity-sdk/skills/google-antigravity-sdk");
    let skill_path_str = skill_path.to_string_lossy().to_string();

    println!("  Loading skills from: {}", skill_path_str);

    let config = LocalConnectionStrategy::default()
        .skills_paths(vec![skill_path_str]);

    let my_agent = Agent::start(config).await?;

    let prompt = "What available skills do you have?";
    println!("  User: {}", prompt);

    let response = my_agent.chat(Some(prompt.into_content())).await?;
    let response_text = response.text().await;
    println!("  Agent: {}", response_text);

    my_agent.stop().await;
    Ok(())
}
