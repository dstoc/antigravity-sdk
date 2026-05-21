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

use antigravity_sdk::{
    Agent, IntoContent, LocalConnectionStrategy,
    types::{
        MultipleChoiceAnswer, QuestionsResponseInner, UserQuestionAnswer, UserQuestionsResponse,
    },
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Default config enables all tools, including ASK_QUESTION.
    let config = LocalConnectionStrategy::default()
        .system_instructions(antigravity_sdk::types::SystemInstructions::custom(
            "When you need clarification or more information from the user to \
             fulfill a request, you should use the `ask_question` tool to prompt them.",
        ))
        .register_on_interaction(
            |_context, spec: antigravity_sdk::types::AskQuestionInteractionSpec| async move {
                println!("\n🙋 [Human-in-the-Loop] The agent requires clarification:");
                let mut answers = Vec::new();

                for (i, question_entry) in spec.questions.iter().enumerate() {
                    println!("  Question {}: {}", i + 1, question_entry.question);
                    if !question_entry.options.is_empty() {
                        for option in &question_entry.options {
                            println!("    [{}] {}", option.id, option.text);
                        }
                    }
                    print!("  Your answer: ");
                    use std::io::Write;
                    std::io::stdout().flush().unwrap();

                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input).unwrap();
                    let trimmed = input.trim().to_string();

                    let answer = UserQuestionAnswer {
                        unanswered: false,
                        multiple_choice_answer: Some(MultipleChoiceAnswer {
                            selected_choice_indices: Vec::new(),
                            freeform_response: Some(trimmed),
                        }),
                    };
                    answers.push(answer);
                }

                Ok(Some(UserQuestionsResponse {
                    trajectory_id: String::new(),
                    step_index: 0,
                    response: QuestionsResponseInner { answers },
                }))
            },
        );

    let my_agent = Agent::start(config).await?;

    // We give the agent an ambiguous prompt to encourage it to ask for clarification.
    let prompt = "I want to search for a file.";
    println!("  User: {}", prompt);

    let response = my_agent.chat(Some(prompt.into_content())).await?;
    let response_text = response.text().await;
    println!("  Agent: {}", response_text);

    my_agent.stop().await;
    Ok(())
}
