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

//! Example demonstrating observability features in Google Antigravity SDK.
//!
//! This example shows how to:
//! - Enable standard logging for the SDK.
//! - Use hooks to create a basic audit log of tool calls.
//! - Access token usage metadata, including thinking tokens.

use std::sync::Arc;
use futures_util::StreamExt;
use antigravity_sdk::{
    Agent, LocalConnectionStrategy, CustomTool, ToolContext, ToolFuture, IntoContent,
};

// A simple tool to demonstrate tool call hooks
struct GetWeather;

impl CustomTool for GetWeather {
    fn name(&self) -> &str {
        "get_weather"
    }

    fn description(&self) -> &str {
        "Gets the weather for a location.\n\nArgs:\n    location: The name of the location."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "location": {
                    "type": "string",
                    "description": "The name of the location."
                }
            },
            "required": ["location"]
        })
    }

    fn call(&self, args: serde_json::Value, _ctx: Option<ToolContext>) -> ToolFuture {
        Box::pin(async move {
            let location = args.get("location")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing location".to_string())?;
            let res = format!("The weather in {} is sunny.", location);
            Ok(serde_json::Value::String(res))
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Enable standard logging for the SDK.
    // In Rust, log configuration is typically driven by the RUST_LOG env var.
    // We set it to 'debug' if it is not already set.
    if std::env::var("RUST_LOG").is_err() {
        unsafe {
            std::env::set_var("RUST_LOG", "debug");
        }
    }
    env_logger::init();

    // Register GetWeather tool and a post_tool_call hook to audit tool executions.
    let config = LocalConnectionStrategy::default()
        .custom_tools(vec![Arc::new(GetWeather) as Arc<dyn CustomTool>])
        .register_post_tool_call(|_context, result| async move {
            println!("\n  [AUDIT] Tool execution completed. Result: {:?}", result);
            Ok(())
        });

    let my_agent = Agent::start(config).await?;
    let prompt = "What is the weather in Seattle?";
    println!("  User: {}", prompt);

    let response = my_agent.chat(Some(prompt.into_content())).await?;

    // Stream the response to stdout
    print!("  Agent: ");
    std::io::Write::flush(&mut std::io::stdout())?;
    let mut stream = response.text_stream();
    while let Some(chunk) = stream.next().await {
        print!("{}", chunk);
        std::io::Write::flush(&mut std::io::stdout())?;
    }
    println!();

    // Access token usage
    let usage = my_agent.conversation().total_usage().await;
    println!("\n  --- Token Usage ---");
    println!("  Prompt tokens: {}", usage.prompt_token_count.unwrap_or(0));
    println!("  Output tokens: {}", usage.candidates_token_count.unwrap_or(0));
    println!("  Thinking tokens: {}", usage.thoughts_token_count.unwrap_or(0));
    println!("  Total tokens: {}", usage.total_token_count.unwrap_or(0));

    my_agent.stop().await;
    Ok(())
}
