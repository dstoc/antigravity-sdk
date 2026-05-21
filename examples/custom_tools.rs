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

//! Example demonstrating custom tools and stateful tools with ToolContext.

use antigravity_sdk::{
    Agent, CustomTool, IntoContent, LocalConnectionStrategy, ToolContext, ToolFuture, allow,
    deny_all,
    types::{CustomSystemInstructionPart, CustomSystemInstructions, SystemInstructions},
};
use std::sync::Arc;

// 1. Define a simple tool
struct LookupFruitSku;

impl CustomTool for LookupFruitSku {
    fn name(&self) -> &str {
        "lookup_fruit_sku"
    }

    fn description(&self) -> &str {
        "Looks up the SKU for a given fruit.\n\nArgs:\n    fruit_name: The name of the fruit.\n\nReturns:\n    A string with the SKU and a simulated order ID for restocking."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "fruit_name": {
                    "type": "string",
                    "description": "The name of the fruit."
                }
            },
            "required": ["fruit_name"]
        })
    }

    fn call(&self, args: serde_json::Value, _ctx: Option<ToolContext>) -> ToolFuture {
        Box::pin(async move {
            let fruit_name = args
                .get("fruit_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing fruit_name".to_string())?;

            let mut skus = std::collections::HashMap::new();
            skus.insert("apple", "SKU-APP-123");
            skus.insert("banana", "SKU-BAN-456");
            skus.insert("orange", "SKU-ORA-789");

            let mut name = fruit_name.to_lowercase();
            if name.ends_with('s') && !skus.contains_key(name.as_str()) {
                name.pop();
            }

            let sku = skus.get(name.as_str()).cloned().unwrap_or("SKU-GEN-000");
            let result = format!(
                "SKU for {} is {}. Order ID for restocking: ORD-{}-NEW",
                fruit_name, sku, sku
            );
            Ok(serde_json::Value::String(result))
        })
    }
}

// 2. Define a stateful tool
struct RecordFruit;

impl CustomTool for RecordFruit {
    fn name(&self) -> &str {
        "record_fruit"
    }

    fn description(&self) -> &str {
        "Records the count of fruits by SKU.\n\nArgs:\n    sku: The SKU of the fruit.\n    count: The number of fruits to record.\n\nReturns:\n    A summary of the current count for that SKU."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "sku": {
                    "type": "string",
                    "description": "The SKU of the fruit."
                },
                "count": {
                    "type": "integer",
                    "description": "The number of fruits to record."
                }
            },
            "required": ["sku", "count"]
        })
    }

    fn call(&self, args: serde_json::Value, ctx: Option<ToolContext>) -> ToolFuture {
        Box::pin(async move {
            let sku = args
                .get("sku")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing sku".to_string())?
                .to_string();

            let count = args
                .get("count")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| "Missing count".to_string())?;

            let ctx = ctx.ok_or_else(|| "Missing ToolContext".to_string())?;

            // Retrieve current counts map or initialize if not present
            let mut counts = if let Some(state_val) = ctx.get_state("fruit_counts").await {
                state_val.as_object().cloned().unwrap_or_default()
            } else {
                serde_json::Map::new()
            };

            let current_count = counts.get(&sku).and_then(|v| v.as_i64()).unwrap_or(0);

            let new_count = current_count + count;
            counts.insert(
                sku.clone(),
                serde_json::Value::Number(serde_json::Number::from(new_count)),
            );

            ctx.set_state("fruit_counts", serde_json::Value::Object(counts))
                .await;

            let result = format!(
                "Recorded {} units for {}. Total count is now {}.",
                count, sku, new_count
            );
            Ok(serde_json::Value::String(result))
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize log levels from environment variables if present
    env_logger::init();

    // Configure the agent connection strategy with both tools.
    let mut config = LocalConnectionStrategy::default();
    config.custom_tools.push(Arc::new(LookupFruitSku));
    config.custom_tools.push(Arc::new(RecordFruit));

    config.system_instructions = Some(SystemInstructions {
        custom: Some(CustomSystemInstructions {
            part: vec![CustomSystemInstructionPart {
                text: "You keep track of fruit inventory. To record fruits, you MUST first look up the fruit's SKU using lookup_fruit_sku, and then use that SKU with record_fruit.".to_string(),
            }],
        }),
        appended: None,
    });

    config.policies = vec![deny_all(), allow("lookup_fruit_sku"), allow("record_fruit")];

    println!("  === Custom Tools Demo ===");
    let my_agent = Agent::start(config).await?;

    // 1. Test simple tool
    let prompt1 = "What is the SKU for apples? We need to order more.";
    println!("\n  User: {}", prompt1);
    let response1 = my_agent.chat(Some(prompt1.into_content())).await?;
    println!("  Agent: {}", response1.text().await);

    // 2. Test stateful tool
    println!("\n  === Stateful Tool (Fruit Counter) Demo ===");
    let turns = vec![
        "I have 5 apples.",
        "And I just got 3 bananas.",
        "Oh, and another 2 apples.",
    ];

    for user_input in turns {
        println!("\n  User: {}", user_input);
        let response = my_agent.chat(Some(user_input.into_content())).await?;
        println!("  Agent: {}", response.text().await);
    }

    my_agent.stop().await;
    Ok(())
}
