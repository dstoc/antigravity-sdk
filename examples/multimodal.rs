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

//! Multimodal example for Google Antigravity SDK.
//!
//! This example demonstrates:
//! - Multimodal input: Passing images and documents to the agent.
//! - Multimodal output: Enabling the agent to generate images.

use antigravity_sdk::types::{BuiltinTools, CapabilitiesConfig, ContentPrimitive, Media};
use antigravity_sdk::{Agent, LocalConnectionStrategy};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Setup paths to resources
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let resources_dir = Path::new(manifest_dir).join("3p/antigravity-sdk/examples/resources");
    let image_path = resources_dir.join("example_image.png");
    let doc_path = resources_dir.join("sample_doc.txt");

    // Multimodal Input: Image
    println!("  --- Multimodal Input: Image ---");
    let agent = Agent::start(LocalConnectionStrategy::default()).await?;

    let image = Media::from_file(&image_path)?;
    let prompt = vec![
        ContentPrimitive::Text("What is in this image?".to_string()),
        ContentPrimitive::Media(image),
    ];
    println!("  User: What is in this image?");
    let response = agent.chat(Some(prompt)).await?;
    println!("  Agent: {}\n", response.text().await);

    // Multimodal Input: Document
    println!("  --- Multimodal Input: Document ---");
    let agent = Agent::start(LocalConnectionStrategy::default()).await?;
    let doc = Media::from_file(&doc_path)?;
    let prompt = vec![
        ContentPrimitive::Text("Summarize this document".to_string()),
        ContentPrimitive::Media(doc),
    ];
    println!("  User: Summarize this document");
    let response = agent.chat(Some(prompt)).await?;
    println!("  Agent: {}\n", response.text().await);

    // Multimodal Output: Image Generation
    println!("  --- Multimodal Output: Image Generation ---");
    let capabilities = CapabilitiesConfig {
        enabled_tools: Some(vec![BuiltinTools::GenerateImage]),
        ..Default::default()
    };
    let gen_config = LocalConnectionStrategy::default().capabilities(capabilities);

    let gen_agent = Agent::start(gen_config).await?;
    let prompt = vec![ContentPrimitive::Text(
        "Generate an image of a futuristic city, name it 'future_city'. \
         Please provide the file path to the generated image."
            .to_string(),
    )];
    println!(
        "  User: Generate an image of a futuristic city, name it 'future_city'. Please provide the file path to the generated image."
    );
    let response = gen_agent.chat(Some(prompt)).await?;
    println!("  Agent: {}\n", response.text().await);

    Ok(())
}
