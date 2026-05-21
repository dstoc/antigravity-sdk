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

//! Example demonstrating background triggers in Google Antigravity SDK.
//!
//! Triggers are background async tasks that run alongside an active agent session.
//! They react to external events (such as timers, file changes, or webhooks) and
//! push automated trigger notifications back to the agent connection.

use antigravity_sdk::{Agent, IntoContent, LocalConnectionStrategy};
use std::sync::{Arc, Mutex};

async fn run_periodic_trigger_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("  === Support Queue Trigger Demo ===");
    println!("  Creating agent and starting session...");

    let config = LocalConnectionStrategy::default().system_instructions(
        antigravity_sdk::types::SystemInstructions::custom(
            "You are a system operations and support assistant. You monitor a \
             queue of incoming support tickets. When the user asks for updates, \
             you must check and report any tickets that came in from the \
             background system alert trigger.",
        ),
    );

    let my_agent = Agent::start(config).await?;
    let conversation = my_agent.conversation().clone();

    // Turn 1: Instruct the agent to watch.
    let prompt1 = "Your task will be to standby and simply let me know if there are any critical tickets received.";
    println!("\n  User: {}", prompt1);
    let response1 = my_agent.chat(Some(prompt1.into_content())).await?;
    println!("  Agent: {}", response1.text().await);

    // Turn 1 is resolved. Spawn background support queue poller.
    let standby_active = Arc::new(Mutex::new(true)); // simulated standby activation
    let poller_active = standby_active.clone();

    let poller_task = tokio::spawn(async move {
        // Sleep 2 seconds then trigger
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if *poller_active.lock().unwrap() {
            println!("\n  [TRIGGER EVENT] Alert! New ticket detected in the queue...");
            if let Err(e) = conversation.send_trigger_notification(
                "[SYSTEM ALERT] New critical ticket assigned: b/98765. Title: Database Connection Leak in Prod."
            ).await {
                eprintln!("Failed to send trigger notification: {}", e);
            }
        }
    });

    println!("\n  Sleeping for 5 seconds. A new ticket will be simulated in the background...");
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // Turn 2: Ask for updates.
    let prompt2 = "I'm back. Did anything critical come in while I was working?";
    println!("\n  User: {}", prompt2);
    let response2 = my_agent.chat(Some(prompt2.into_content())).await?;
    println!("  Agent: {}", response2.text().await);

    // End session
    *standby_active.lock().unwrap() = false;
    poller_task.await?;
    my_agent.stop().await;
    println!("\n  Ending session. Background triggers will stop automatically.");
    Ok(())
}

async fn run_custom_trigger_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("  === Custom Webhook Trigger Demo ===");
    println!("  Creating agent and starting session...");

    let config = LocalConnectionStrategy::default().system_instructions(
        antigravity_sdk::types::SystemInstructions::custom(
            "You are a CI/CD operations assistant. You monitor pipeline status \
             via an external webhook trigger. When the user asks for updates, \
             you must check and report any failures that came in from the \
             webhook alert trigger.",
        ),
    );

    let my_agent = Agent::start(config).await?;
    let conversation = my_agent.conversation().clone();

    // Turn 1: Set standby monitoring.
    let prompt1 = "Your task will be to standby and simply let me know if there are any critical pipeline webhook alerts received.";
    println!("\n  User: {}", prompt1);
    let response1 = my_agent.chat(Some(prompt1.into_content())).await?;
    println!("  Agent: {}", response1.text().await);

    // Spawn custom webhook listener background task
    let webhook_active = Arc::new(Mutex::new(true));
    let listener_active = webhook_active.clone();

    let webhook_task = tokio::spawn(async move {
        println!("\n  [WEBHOOK TRIGGER] Custom Webhook listener started...");
        // Wait 3 seconds, then trigger simulated failure event
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        if *listener_active.lock().unwrap() {
            println!("\n  [WEBHOOK TRIGGER] Event received: 'AppBuild-42' status FAILED.");
            if let Err(e) = conversation.send_trigger_notification(
                "[WEBHOOK ALERT] CI/CD Build Pipeline 'AppBuild-42' FAILED on branch 'main'. Reason: Lint errors in routes.py."
            ).await {
                eprintln!("Failed to send trigger notification: {}", e);
            }
        }
    });

    println!(
        "\n  Sleeping for 5 seconds. A pipeline failure will be simulated in the background..."
    );
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // Turn 2: Ask for updates.
    let prompt2 = "I'm back. Any updates on my builds?";
    println!("\n  User: {}", prompt2);
    let response2 = my_agent.chat(Some(prompt2.into_content())).await?;
    println!("  Agent: {}", response2.text().await);

    // End session
    *webhook_active.lock().unwrap() = false;
    webhook_task.await?;
    my_agent.stop().await;
    println!("\n  Ending session. Background triggers will stop automatically.");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    run_periodic_trigger_example().await?;
    println!("\n============================================================\n");
    run_custom_trigger_example().await?;

    Ok(())
}
