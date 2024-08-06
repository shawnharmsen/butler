// /path/to/your/project/src/main.rs

use anyhow::{Context, Result};
use clap::Parser;
use colored::*;
use config::{Config, Environment, File};
use dotenv::dotenv;
use indicatif::{ProgressBar, ProgressStyle};
use log::{error, info, warn};
use reqwest::{Client, Response};
use rustyline::{DefaultEditor, Result as RustylineResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::PathBuf, time::Duration};
use tokio::time::Instant;

#[derive(Parser)]
#[clap(version = "1.0", author = "Dodger")]
struct Opts {
    #[clap(short, long)]
    prompt: Option<String>,
    #[clap(short, long, default_value = "anthropic/claude-3.5-sonnet")]
    model: String,
    #[clap(short, long)]
    config: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Message {
    role: String,
    content: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<Message>,
    stream: bool,
}

#[derive(Deserialize, Debug)]
struct ChatCompletionResponse {
    model: String,
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
    last_request_time: Option<Instant>,
    rate_limit: Duration,
}

impl OpenAI {
    fn new(api_key: String, site_url: String, site_name: String, rate_limit: Duration) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: "https://openrouter.ai/api/v1".to_string(),
            site_url,
            site_name,
            last_request_time: None,
            rate_limit,
        }
    }

    async fn create_chat_completion(&mut self, request: &ChatCompletionRequest) -> Result<Response> {
        self.apply_rate_limit().await;

        let url = format!("{}/chat/completions", self.base_url);

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", &self.site_url)
            .header("X-Title", &self.site_name)
            .header("Accept", "application/json")
            .json(request)
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

    async fn apply_rate_limit(&mut self) {
        if let Some(last_request_time) = self.last_request_time {
            let elapsed = last_request_time.elapsed();
            if elapsed < self.rate_limit {
                tokio::time::sleep(self.rate_limit - elapsed).await;
            }
        }
        self.last_request_time = Some(Instant::now());
    }
}

#[derive(Deserialize, Debug)]
struct AppConfig {
    openrouter_api_key: String,
    your_site_url: String,
    your_site_name: String,
    app_rate_limit_ms: u64,
}


fn load_config(config_path: Option<PathBuf>) -> Result<AppConfig> {
    let mut builder = Config::builder()
        .add_source(File::with_name("config/default").required(false))
        .add_source(Environment::default());

    if let Some(path) = config_path {
        builder = builder.add_source(File::from(path));
    }

    let config = builder.build()?;
    config.try_deserialize().context("Failed to parse configuration")
}


async fn process_stream(mut response: Response) -> Result<()> {
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let text = String::from_utf8_lossy(&chunk);
        buffer.push_str(&text);

        if buffer.contains("\n\n") {
            let parts: Vec<&str> = buffer.split("\n\n").collect();
            for part in parts.iter().take(parts.len() - 1) {
                if let Some(content) = process_sse_message(part) {
                    print!("{}", content.green());
                }
            }
            buffer = parts.last().unwrap().to_string();
        }
    }

    if !buffer.is_empty() {
        if let Some(content) = process_sse_message(&buffer) {
            print!("{}", content.green());
        }
    }

    println!();
    Ok(())
}

fn process_sse_message(message: &str) -> Option<String> {
    if let Some(data) = message.strip_prefix("data: ") {
        if data.trim() == "[DONE]" {
            return None;
        }
        if let Ok(json) = serde_json::from_str::<Value>(data) {
            if let Some(content) = json["choices"][0]["delta"]["content"].as_str() {
                return Some(content.to_string());
            }
        }
    }
    None
}



#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    env_logger::init();

    let opts: Opts = Opts::parse();
    let config = load_config(opts.config)?;

    let mut openai = OpenAI::new(
        config.openrouter_api_key,
        config.your_site_url,
        config.your_site_name,
        Duration::from_millis(config.app_rate_limit_ms),
    );


    let mut rl = DefaultEditor::new()?;
    let mut conversation_history: Vec<Message> = Vec::new();

    info!("Welcome to the interactive AI assistant. Type 'exit' to quit.");
    println!("{}", "Welcome to the interactive AI assistant. Type 'exit' to quit.".cyan());
    println!("{}", "-------------------------------------------------------------".cyan());

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ")
            .template("{spinner} Waiting for AI response...").unwrap()
    );

    while let RustylineResult::Ok(line) = rl.readline("You: ") {
        if line.trim().to_lowercase() == "exit" {
            info!("Exiting the application");
            println!("{}", "Goodbye!".cyan());
            break;
        }

        if let Err(err) = rl.add_history_entry(&line) {
            warn!("Failed to add history entry: {}", err);
        }

        conversation_history.push(Message {
            role: "user".to_string(),
            content: line.clone(),
        });

        let request = ChatCompletionRequest {
            model: opts.model.clone(),
            messages: conversation_history.clone(),
            stream: true,
        };

        pb.enable_steady_tick(Duration::from_millis(120));

        match openai.create_chat_completion(&request).await {
            Ok(response) => {
                pb.finish_and_clear();

                if request.stream {
                    print!("\n{}: ", "AI".blue());
                    if let Err(e) = process_stream(response).await {
                        error!("Error processing stream: {}", e);
                    }
                } else {
                    let chat_response: ChatCompletionResponse = response.json().await?;
                    info!("Received response from model: {}", chat_response.model);
                    println!("\n{}", "Metadata:".yellow());
                    println!("  Model: {}", chat_response.model.green());
                    println!("  Tokens: {} prompt, {} completion, {} total",
                             chat_response.usage.prompt_tokens.to_string().green(),
                             chat_response.usage.completion_tokens.to_string().green(),
                             chat_response.usage.total_tokens.to_string().green()
                    );

                    if let Some(choice) = chat_response.choices.first() {
                        println!("\n{}: {}", "AI".blue(), choice.message.content.green());
                        conversation_history.push(Message {
                            role: "assistant".to_string(),
                            content: choice.message.content.clone(),
                        });
                    } else {
                        println!("\n{}: {}", "AI".blue(), "No response content available.".red());
                    }
                }
                println!("{}", "-------------------------------------------------------------".cyan());
            }
            Err(e) => {
                pb.finish_and_clear();
                error!("Error: {}", e);
                eprintln!("{}: {}", "Error".red(), e);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::mock;
    use tokio;

    #[tokio::test]
    async fn test_create_chat_completion() {
        let mock_server = mock("POST", "/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"model":"test-model","choices":[{"message":{"content":"Test response"}}],"usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30}}"#)
            .create();

        let mut openai = OpenAI::new(
            "test_api_key".to_string(),
            "http://test.com".to_string(),
            "Test App".to_string(),
            Duration::from_millis(100),
        );
        openai.base_url = mockito::server_url();

        let request = ChatCompletionRequest {
            model: "test-model".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: "Test message".to_string(),
            }],
            stream: false,
        };

        let response = openai.create_chat_completion(&request).await.unwrap();
        let chat_response: ChatCompletionResponse = response.json().await.unwrap();

        assert_eq!(chat_response.model, "test-model");
        assert_eq!(chat_response.choices[0].message.content, "Test response");
        assert_eq!(chat_response.usage.total_tokens, 30);

        mock_server.assert();
    }
}
