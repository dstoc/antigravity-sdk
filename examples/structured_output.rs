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

//! Example demonstrating native structured output from an agent.
//!
//! This example shows how to configure the agent to return a strongly-typed,
//! validated JSON payload instead of raw, unstructured conversational text.

use antigravity_sdk::types::CapabilitiesConfig;
use antigravity_sdk::{
    Agent, CustomTool, IntoContent, LocalConnectionStrategy, ToolContext, ToolFuture,
};
use std::sync::Arc;

// A custom mock tool that retrieves unstructured text data
struct FetchUnstructuredMeetingNotes;

impl CustomTool for FetchUnstructuredMeetingNotes {
    fn name(&self) -> &str {
        "fetch_unstructured_meeting_notes"
    }

    fn description(&self) -> &str {
        "Retrieves the raw unstructured notes for a given meeting ID.\n\nArgs:\n    meeting_id: The ID of the meeting."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "meeting_id": {
                    "type": "string",
                    "description": "The ID of the meeting."
                }
            },
            "required": ["meeting_id"]
        })
    }

    fn call(&self, args: serde_json::Value, _ctx: Option<ToolContext>) -> ToolFuture {
        Box::pin(async move {
            let meeting_id = args
                .get("meeting_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing meeting_id".to_string())?;
            let res = if meeting_id == "meeting-2026-05" {
                "Discussed launch timeline for project X. Alice agreed to update \
                 the textproto tests by Monday. Bob mentioned he will run the final \
                 E2E benchmarks tomorrow. I will push the release build once the \
                 tests are green."
            } else {
                "Error: Meeting notes not found."
            };
            Ok(serde_json::Value::String(res.to_string()))
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    println!("  --- Starting main ---");

    // Define JSON schema for the MeetingSummary response schema.
    let response_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "action_items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "assignee": {
                            "type": "string"
                        },
                        "task": {
                            "type": "string"
                        },
                        "deadline": {
                            "type": "string"
                        }
                    },
                    "required": ["assignee", "task", "deadline"]
                }
            }
        },
        "required": ["action_items"]
    });

    let capabilities = CapabilitiesConfig {
        finish_tool_schema_json: Some(response_schema.to_string()),
        ..Default::default()
    };

    let config = LocalConnectionStrategy::default()
        .capabilities(capabilities)
        .custom_tools(vec![
            Arc::new(FetchUnstructuredMeetingNotes) as Arc<dyn CustomTool>
        ]);

    let meeting_agent = Agent::start(config).await?;

    let prompt = "Use the fetch_unstructured_meeting_notes tool to retrieve notes for \
                  'meeting-2026-05' and return the meeting summary with the appropriate \
                  action item list. Ensure each action item includes 'assignee', \
                  'task', and 'deadline'.";

    println!("\n  Sending prompt to agent...");
    let response = meeting_agent.chat(Some(prompt.into_content())).await?;

    println!("\n  Extracting structured meeting action items...");
    let data = response.structured_output().await;

    if data.is_none() {
        println!("\n  Failed to extract structured summary natively.");
        println!("  Final Text Response: {}", response.text().await);
        meeting_agent.stop().await;
        return Ok(());
    }

    let data = data.unwrap();
    println!("\n  === Structured Meeting Action Items ===");
    if let Some(action_items) = data.get("action_items").and_then(|v| v.as_array()) {
        for item in action_items {
            println!(
                "  - Assignee: {}",
                item.get("assignee").and_then(|v| v.as_str()).unwrap_or("")
            );
            println!(
                "    Task:     {}",
                item.get("task").and_then(|v| v.as_str()).unwrap_or("")
            );
            println!(
                "    Deadline: {}\n",
                item.get("deadline").and_then(|v| v.as_str()).unwrap_or("")
            );
        }
    }

    meeting_agent.stop().await;

    Ok(())
}
