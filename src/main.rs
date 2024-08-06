#![allow(non_snake_case)]
// /path/to/your/project/src/main.rs

// src/main.rs
use dotenv::dotenv;
use anyhow::{Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use rustyline::DefaultEditor;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use std::time::Duration;


#[derive(Parser)]
#[clap(version = "1.0", author = "Dodger")]
struct Opts {
    #[clap(short, long)]
    prompt: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Message {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<Message>,
}

#[derive(Deserialize, Debug)]
struct ChatCompletionResponse {
    id: String,
    model: String,
    object: String,
    created: u64,
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Deserialize, Debug)]
struct Choice {
    message: AssistantMessage,
}

#[derive(Deserialize, Debug)]
struct AssistantMessage {
    content: String,
}

#[derive(Deserialize, Debug)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

struct OpenAI {
    client: Client,
    api_key: String,
    base_url: String,
    site_url: String,
    site_name: String,
}

impl OpenAI {
    fn new(api_key: String, site_url: String, site_name: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: "https://openrouter.ai/api/v1".to_string(),
            site_url,
            site_name,
        }
    }

    async fn create_chat_completion(&self, request: ChatCompletionRequest) -> Result<reqwest::Response> {
        let url = format!("{}/chat/completions", self.base_url);

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", &self.site_url)
            .header("X-Title", &self.site_name)
            .header("Accept", "text/event-stream")
            .json(&request)
            .send()
            .await
            .context("Failed to send request to OpenRouter")?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.context("Failed to get error response body")?;
            anyhow::bail!("OpenRouter API error: Status {}, Body: {}", status, error_body);
        }

        Ok(response)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    let api_key = env::var("OPENROUTER_API_KEY")
        .context("OPENROUTER_API_KEY must be set in .env file")?;
    let site_url = env::var("YOUR_SITE_URL")
        .context("YOUR_SITE_URL must be set in .env file")?;
    let site_name = env::var("YOUR_SITE_NAME")
        .context("YOUR_SITE_NAME must be set in .env file")?;

    println!("Welcome to the interactive AI assistant. Type 'exit' to quit.");
    println!("-------------------------------------------------------------");

    loop {
        let readline = rl.readline("You: ");
        match readline {
            Ok(line) => {
                if line.trim().to_lowercase() == "exit" {
                    println!("Goodbye!");
                    break;
                }

                if let Err(err) = rl.add_history_entry(line.as_str()) {
                    eprintln!("Warning: Failed to add history entry: {}", err);
                }

                let request = ChatCompletionRequest {
                    model: "anthropic/claude-3.5-sonnet".to_string(),
                    messages: vec![Message {
                        role: "user".to_string(),
                        content: line,
                    }],
                };

                let pb = ProgressBar::new_spinner();
                pb.set_style(ProgressStyle::default_spinner()
                    .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ")
                    .template("{spinner} Waiting for AI response...").unwrap());
                pb.enable_steady_tick(Duration::from_millis(120));

                match openai.create_chat_completion(request).await {
                    Ok(response) => {
                        pb.finish_and_clear();

                        let body = response.text().await?;

                        match serde_json::from_str::<ChatCompletionResponse>(&body) {
                            Ok(parsed_response) => {
                                let metadata = json!({
                                    "id": parsed_response.id,
                                    "model": parsed_response.model,
                                    "object": parsed_response.object,
                                    "created": parsed_response.created,
                                    "prompt_tokens": parsed_response.usage.prompt_tokens,
                                    "completion_tokens": parsed_response.usage.completion_tokens,
                                    "total_tokens": parsed_response.usage.total_tokens
                                });

                                println!("\nMetadata: {}", serde_json::to_string_pretty(&metadata)?);

                                if let Some(choice) = parsed_response.choices.first() {
                                    println!("\nAI: {}", choice.message.content);
                                } else {
                                    println!("\nAI: No response content available.");
                                }
                                println!("-------------------------------------------------------------");
                            }
                            Err(e) => {
                                eprintln!("Failed to parse JSON: {}", e);
                                println!("\nAI: Sorry, I encountered an error processing the response.");
                            }
                        }
                    }
                    Err(e) => {
                        pb.finish_and_clear();
                        println!("Error: {}", e);
                    }
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted) => {
                println!("CTRL-C");
                break;
            }
            Err(rustyline::error::ReadlineError::Eof) => {
                println!("CTRL-D");
                break;
            }
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
    }

    Ok(())
}


