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

//! Example demonstrating system instructions in Google Antigravity SDK.
//!
//! This example shows how to configure the agent's system instructions using both
//! templated and custom approaches.

use std::path::Path;
use std::sync::Arc;
use antigravity_sdk::types::{SystemInstructionSection, SystemInstructions};
use antigravity_sdk::{
    Agent, CustomTool, IntoContent, LocalConnectionStrategy, ToolContext, ToolFuture,
};

struct CheckStyleGuide;

impl CustomTool for CheckStyleGuide {
    fn name(&self) -> &str {
        "check_style_guide"
    }

    fn description(&self) -> &str {
        "Checks the style guide rules for a given language.\n\nArgs:\n    language: The programming language."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "language": {
                    "type": "string",
                    "description": "The programming language."
                }
            },
            "required": ["language"]
        })
    }

    fn call(&self, args: serde_json::Value, _ctx: Option<ToolContext>) -> ToolFuture {
        Box::pin(async move {
            let language = args.get("language")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing language".to_string())?;
            let result = if language.to_lowercase() == "python" {
                "Use snake_case for functions and variables. Use CamelCase for classes."
            } else {
                "No specific rules found."
            };
            Ok(serde_json::Value::String(result.to_string()))
        })
    }
}

async fn run_templated_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("  === Templated System Instructions Example ===");

    // Override the Identity (Persona)
    let identity = "You are an expert Code Quality Reviewer.\n\
                    Your role is to review code for readability, maintainability, and adherence to style guides.";

    // Add custom sections
    let review_criteria = SystemInstructionSection::new(
        "review_criteria",
        "- Focus on readability and simplicity.\n\
         - Ensure meaningful variable and function names.",
    );

    let style_guide_instructions = SystemInstructionSection::new(
        "style_guide_instructions",
        "When reviewing Python code, use the `check_style_guide` tool to verify rules.",
    );

    let templated_si = SystemInstructions::templated(
        identity,
        vec![review_criteria, style_guide_instructions],
    );

    let config = LocalConnectionStrategy::default()
        .system_instructions(templated_si)
        .custom_tools(vec![Arc::new(CheckStyleGuide) as Arc<dyn CustomTool>]);

    let my_agent = Agent::start(config).await?;
    let prompt = "Review this Python code: `def MY_FUNCTION(X): return X*2`";
    println!("  User: {}", prompt);
    let response = my_agent.chat(Some(prompt.into_content())).await?;
    println!("  Agent: {}\n", response.text().await);
    my_agent.stop().await;

    Ok(())
}

fn build_skills_instructions(skills_paths: &[String]) -> String {
    if skills_paths.is_empty() {
        return String::new();
    }

    let mut instructions = "\n<skills>\n".to_string();
    instructions.push_str("Skills enhance your abilities with specialized expertise and repeatable workflows to help solve advanced workflows.\n");
    instructions.push_str("When a task matches an available skill's description, you must inspect the complete SKILL.md with your 'view_file' tool in order to understand its capabilities.\n\n");
    instructions.push_str("Available skills:\n");
    for path in skills_paths {
        let skill_name = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        instructions.push_str(&format!(
            "* **{}** (located at `{}/SKILL.md`) — Provides guidelines for code readability, style compliance, and refactoring.\n",
            skill_name, path
        ));
    }
    instructions.push_str("</skills>\n");
    instructions
}

async fn run_custom_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("  === Custom System Instructions Example ===");

    // Static Identity/Persona
    let identity_text = "
<identity>
You are an expert Code Quality Reviewer agent. Your goal is to help developers maintain high standards of readability, maintainability, and correctness in their code. You will receive code snippets or descriptions of code changes and provide actionable feedback. You must always prioritize addressing the user's specific questions or concerns about the code.
</identity>
";

    // Dynamically gather workspace and app data directory info in Rust.
    let cwd = std::env::current_dir().unwrap_or_default().to_string_lossy().into_owned();
    let app_data_dir = "~/.gemini/antigravity".to_string();
    let os_name = std::env::consts::OS;
    let user_info = format!("
<user_information>
Operating System: {os_name}
Active Workspace CWD: {cwd}
Storage Directory (App Data): {app_data_dir}
</user_information>
");

    // Configure the active skill folders.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let skill_path = Path::new(manifest_dir)
        .join("skills/google-antigravity-sdk")
        .to_string_lossy()
        .into_owned();
    let skills = vec![skill_path];
    let skills_instructions = build_skills_instructions(&skills);

    // Standard structured guidelines & formatting rules text
    let guidelines_text = "
<review_guidelines>
### When to recommend refactoring:
- The code has high cyclomatic complexity (too many nested loops/conditionals).
- The code violates DRY (Don't Repeat Yourself) principles significantly.
- The code is difficult to unit test in its current form.

### Don't recommend refactoring for:
- Minor personal style preferences that don't impact readability.
- Micro-optimizations that make the code harder to understand.
</review_guidelines>

<task_management>
### When to suggest breaking up the review:
- If the provided code snippet is longer than 200 lines.
- If the user is asking for both a security audit and a performance review at the same time.
In these cases, suggest reviewing one specific aspect or file first.
</task_management>

<behavioral_principles>
1. **Acknowledge Ambiguity**: If a request is underspecified or could be interpreted in multiple ways, ask the user for clarification before proceeding.
2. **Precision**: When suggesting code changes, always specify the file path and, if applicable, the line range.
3. **Focus on Delta**: Do not restate full file contents or large blocks of code unless necessary. Focus only on what needs to change.
4. **Closure**: End every turn with a clear summary of what was accomplished and what the next steps are.
</behavioral_principles>

<review_artifact_format>
When generating a detailed review artifact in Markdown, use the following elements to ensure high quality and scannability:

### Alerts
Use GitHub-style alerts to highlight critical issues:
> [!IMPORTANT]
> Critical security or correctness issues that must be fixed.

> [!NOTE]
> General improvements or style suggestions.

### Code Diffs
When suggesting changes, use diff blocks to show exactly what to add or remove:
```diff
-def old_func():
+def new_func():
```

### Tables
Use tables to compare alternative approaches or list multiple findings:
| File | Line | Issue | Severity |
| :--- | :--- | :--- | :--- |
| main.py | 12 | Hardcoded API key | Critical |
</review_artifact_format>

<tool_usage>
You have access to the `check_style_guide` tool. When reviewing Python code, always use this tool to verify language-specific style rules before making recommendations.
</tool_usage>
";

    // Assemble the finalized custom system prompt string
    let final_si_prompt = format!("{}{}{}{}", identity_text, skills_instructions, guidelines_text, user_info);
    let custom_si = SystemInstructions::custom(final_si_prompt);

    let config = LocalConnectionStrategy::default()
        .system_instructions(custom_si)
        .custom_tools(vec![Arc::new(CheckStyleGuide) as Arc<dyn CustomTool>])
        .skills_paths(skills);

    let my_agent = Agent::start(config).await?;
    let prompt = "Review this Python code: `def foo(x): return x+1`";
    println!("  User: {}", prompt);
    let response = my_agent.chat(Some(prompt.into_content())).await?;
    println!("  Agent: {}\n", response.text().await);
    my_agent.stop().await;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    run_templated_example().await?;
    run_custom_example().await?;

    Ok(())
}
