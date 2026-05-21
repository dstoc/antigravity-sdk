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

use std::sync::Arc;
use antigravity_sdk::{Agent, LocalConnectionStrategy, IntoContent, CustomTool, ToolContext, ToolFuture};

struct ExplodingTool;

impl CustomTool for ExplodingTool {
    fn name(&self) -> &str {
        "exploding_tool"
    }

    fn description(&self) -> &str {
        "A tool that always fails, regardless of input.\n\nArgs:\n    input_data: Any string input."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "input_data": {
                    "type": "string",
                    "description": "Any string input."
                }
            },
            "required": ["input_data"]
        })
    }

    fn call(&self, args: serde_json::Value, _ctx: Option<ToolContext>) -> ToolFuture {
        Box::pin(async move {
            let input_data = args.get("input_data")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            println!("\n  🔧 [Tool] Exploding tool called with: {}, exploding...", input_data);
            Err("This tool is intentionally broken and always fails.".to_string())
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    println!("  🔌 Error Handling Example\n");

    let config = LocalConnectionStrategy::default()
        .custom_tools(vec![Arc::new(ExplodingTool)])
        .register_on_tool_error(|_context, error_msg: String| async move {
            println!("\n  🔧 [ErrorHandler] Caught exception: {}", error_msg);
            // Return a message that the model will see instead of the raw error.
            // This guides the model on how to respond or recover.
            Ok(Some(serde_json::json!(format!(
                "[Tool Error: {} Please inform the user that the operation failed.]",
                error_msg
            ))))
        });

    let my_agent = Agent::start(config).await?;

    let prompt = "Use the exploding_tool with input 'test data'.";
    println!("  User: {}", prompt);

    match my_agent.chat(Some(prompt.into_content())).await {
        Ok(response) => {
            let response_text = response.text().await;
            println!("  Agent: {}", response_text);
        }
        Err(e) => {
            println!("\n  [App Error] SDK turn failed: {}", e);
        }
    }

    my_agent.stop().await;
    Ok(())
}
