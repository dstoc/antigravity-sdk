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

    // Create a temporary directory for the custom application data storage
    let temp_dir = tempfile::tempdir()?;
    let custom_app_data = temp_dir.path().to_path_buf();
    let custom_app_data_str = custom_app_data.to_string_lossy().to_string();
    println!("  Custom App Data Dir: {}\n", custom_app_data_str);

    // Initialize the agent config with our custom app_data_dir override
    let config = LocalConnectionStrategy::default()
        .app_data_dir(custom_app_data_str);

    // Start the agent and ask it to create an artifact
    let my_agent = Agent::start(config).await?;
    let conv_id = my_agent.conversation_id();
    println!("  Agent Session Started. Conversation ID: {}\n", conv_id);

    let prompt = "Please create an artifact file named 'rust_best_practices.md' summarizing Rust best practices.";
    println!("  User:  {}", prompt);
    let response = my_agent.chat(Some(prompt.into_content())).await?;
    println!("  Agent: {}\n", response.text().await);

    // Verify that the artifact was successfully stored in our custom app_data_dir
    let expected_artifact_path = custom_app_data
        .join("brain")
        .join(&conv_id)
        .join("rust_best_practices.md");

    println!("  Checking artifact location: {:?}", expected_artifact_path);
    if expected_artifact_path.exists() {
        println!("\n  SUCCESS: Verified artifact successfully stored in custom app_data_dir!");
    } else {
        println!("\n  WARNING: Artifact was not found in custom app_data_dir.");
    }

    my_agent.stop().await;
    Ok(())
}
