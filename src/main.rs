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
use std::env;
use std::io::{stdout, Write};
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
    choices: Vec<Choice>,
}

#[derive(Deserialize, Debug)]
struct Choice {
    message: AssistantMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
struct AssistantMessage {
    role: String,
    content: String,
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

    let openai = OpenAI::new(api_key, site_url, site_name);
    let mut rl = DefaultEditor::new()?;

    println!("Welcome to the interactive AI assistant. Type 'exit' to quit.");

    loop {
        let readline = rl.readline("Question: ");
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
                    .template("{spinner} Thinking...").unwrap());
                pb.enable_steady_tick(Duration::from_millis(120));

                match openai.create_chat_completion(request).await {
                    Ok(response) => {
                        pb.finish_and_clear();

                        let body = response.text().await?;

                        // Debug: Print full response
                        eprintln!("Full response: {}", body);

                        match serde_json::from_str::<ChatCompletionResponse>(&body) {
                            Ok(parsed_response) => {
                                if let Some(choice) = parsed_response.choices.first() {
                                    println!("AI: {}", choice.message.content);
                                } else {
                                    println!("AI: No response content available.");
                                }
                            }
                            Err(e) => {
                                eprintln!("Failed to parse JSON: {}", e);
                                println!("AI: Sorry, I encountered an error processing the response.");
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


