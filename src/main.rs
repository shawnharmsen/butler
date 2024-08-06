use anyhow::{Context, Result};
use clap::Parser;
use colored::*;
use dotenv::dotenv;
use indicatif::{ProgressBar, ProgressStyle};
use log::{error, info, warn};
use reqwest::Client;
use rustyline::{DefaultEditor, Result as RustylineResult};
use serde::{Deserialize, Serialize};
use std::{env, time::Duration, sync::Arc};
use tokio::time::Instant;
use tokio::sync::Mutex;

#[derive(Parser)]
#[clap(version = "1.0", author = "Dodger")]
struct Opts {
    #[clap(short, long)]
    prompt: Option<String>,
    #[clap(short, long, default_value = "anthropic/claude-3.5-sonnet")]
    model: String,
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
    id: String,
    model: String,
    usage: Usage,
    choices: Vec<Choice>,
}

#[derive(Deserialize, Debug)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Deserialize, Debug)]
struct Choice {
    message: Message,
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

    async fn create_chat_completion(&mut self, request: &ChatCompletionRequest) -> Result<ChatCompletionResponse> {
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

        response.json::<ChatCompletionResponse>().await.context("Failed to parse response")
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

fn load_config() -> Result<(String, String, String, u64)> {
    dotenv().ok();

    Ok((
        env::var("OPENROUTER_API_KEY").context("OPENROUTER_API_KEY must be set in .env file")?,
        env::var("YOUR_SITE_URL").context("YOUR_SITE_URL must be set in .env file")?,
        env::var("YOUR_SITE_NAME").context("YOUR_SITE_NAME must be set in .env file")?,
        env::var("APP_RATE_LIMIT_MS")
            .context("APP_RATE_LIMIT_MS must be set in .env file")?
            .parse()
            .context("APP_RATE_LIMIT_MS must be a valid integer")?,
    ))
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let (api_key, site_url, site_name, rate_limit_ms) = load_config()?;

    let opts: Opts = Opts::parse();

    let openai = Arc::new(Mutex::new(OpenAI::new(
        api_key,
        site_url,
        site_name,
        Duration::from_millis(rate_limit_ms),
    )));

    let mut rl = DefaultEditor::new()?;
    let conversation_history = Arc::new(Mutex::new(Vec::new()));

    info!("Welcome to the interactive AI assistant. Type 'exit' to quit.");
    println!("{}", "Welcome to the interactive AI assistant. Type 'exit' to quit.".cyan());
    println!("{}", "-------------------------------------------------------------".cyan());

    let pb = Arc::new(Mutex::new(ProgressBar::new_spinner()));
    pb.lock().await.set_style(ProgressStyle::default_spinner()
        .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ")
        .template("{spinner} Processing...").unwrap());

    let processing = Arc::new(Mutex::new(false));

    while let RustylineResult::Ok(line) = rl.readline("You: ") {
        if line.trim().eq_ignore_ascii_case("exit") {
            info!("Exiting the application");
            println!("{}", "Goodbye!".cyan());
            break;
        }

        if line.trim().is_empty() {
            continue;
        }

        let mut is_processing = processing.lock().await;
        if *is_processing {
            println!("Still processing previous request. Please wait.");
            continue;
        }
        *is_processing = true;
        drop(is_processing);

        if let Err(err) = rl.add_history_entry(&line) {
            warn!("Failed to add history entry: {}", err);
        }

        conversation_history.lock().await.push(Message {
            role: "user".to_string(),
            content: line,
        });

        let request = ChatCompletionRequest {
            model: opts.model.clone(),
            messages: conversation_history.lock().await.clone(),
            stream: false,
        };

        let pb_clone = Arc::clone(&pb);
        let processing_clone = Arc::clone(&processing);
        let openai_clone = Arc::clone(&openai);
        let conversation_history_clone = Arc::clone(&conversation_history);

        tokio::spawn(async move {
            pb_clone.lock().await.enable_steady_tick(Duration::from_millis(100));

            match openai_clone.lock().await.create_chat_completion(&request).await {
                Ok(response) => {
                    pb_clone.lock().await.finish_and_clear();
                    print!("\n{}: ", "AI".blue());
                    if let Some(choice) = response.choices.first() {
                        println!("{}", choice.message.content.green());
                    }
                    println!("\n{}", "Metadata:".yellow());
                    println!("Model: {}", response.model);
                    println!("Tokens: {} prompt, {} completion, {} total",
                             response.usage.prompt_tokens,
                             response.usage.completion_tokens,
                             response.usage.total_tokens);
                    println!("Response ID: {}", response.id);
                    println!("{}", "-------------------------------------------------------------".cyan());

                    conversation_history_clone.lock().await.push(response.choices[0].message.clone());
                }
                Err(e) => {
                    pb_clone.lock().await.finish_and_clear();
                    error!("Error: {}", e);
                    eprintln!("{}: {}", "Error".red(), e);
                }
            }

            let mut is_processing = processing_clone.lock().await;
            *is_processing = false;
        });
    }

    Ok(())
}
