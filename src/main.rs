use anyhow::{Context, Result};
use clap::Parser;
use colored::*;
use config::{Config, Environment, File};
use dotenv::dotenv;
use log::{error, info, warn};
use reqwest::{Client, Response};
use rustyline::{DefaultEditor, Result as RustylineResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::PathBuf, time::Duration};
use tokio::time::Instant;
use futures_util::StreamExt;


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
    let builder = Config::builder()
        .add_source(File::with_name("config/default").required(false))
        .add_source(Environment::default());

    let builder = if let Some(path) = config_path {
        builder.add_source(File::from(path))
    } else {
        builder
    };

    builder.build()?
        .try_deserialize()
        .context("Failed to parse configuration")
}

async fn process_stream(response: Response) -> Result<()> {
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let text = String::from_utf8_lossy(&chunk);
        if let Some(content) = process_sse_message(&text) {
            print!("{}", content.green());
        }
    }

    println!();
    Ok(())
}

fn process_sse_message(message: &str) -> Option<String> {
    message.lines()
        .filter(|line| line.starts_with("data: "))
        .filter_map(|line| {
            let data = &line["data: ".len()..];
            if data.trim() == "[DONE]" {
                None
            } else {
                serde_json::from_str::<Value>(data).ok()
                    .and_then(|json| json["choices"][0]["delta"]["content"].as_str().map(String::from))
            }
        })
        .next()
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

    while let RustylineResult::Ok(line) = rl.readline("You: ") {
        if line.trim().eq_ignore_ascii_case("exit") {
            info!("Exiting the application");
            println!("{}", "Goodbye!".cyan());
            break;
        }

        if let Err(err) = rl.add_history_entry(&line) {
            warn!("Failed to add history entry: {}", err);
        }

        conversation_history.push(Message {
            role: "user".to_string(),
            content: line,
        });

        let request = ChatCompletionRequest {
            model: opts.model.clone(),
            messages: conversation_history.clone(),
            stream: true,
        };

        match openai.create_chat_completion(&request).await {
            Ok(response) => {
                print!("\n{}: ", "AI".blue());
                if let Err(e) = process_stream(response).await {
                    error!("Error processing stream: {}", e);
                }
                println!("{}", "-------------------------------------------------------------".cyan());
            }
            Err(e) => {
                error!("Error: {}", e);
                eprintln!("{}: {}", "Error".red(), e);
            }
        }
    }

    Ok(())
}
