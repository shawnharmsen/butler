// src/main.rs
use indicatif::{ProgressBar, ProgressStyle};
use anyhow::{Context, Result};
use clap::Parser;
use colored::*;
use dotenv::dotenv;
use log::{error, info};
use reqwest::Client;
use rustyline::error::ReadlineError;
use rustyline::{DefaultEditor, Result as RustylineResult};
use serde::{Deserialize, Serialize};
use std::{env, sync::Arc};
use tokio::sync::Semaphore;

#[derive(Parser)]
#[clap(version = "1.0", author = "Dodger")]
struct Opts {
    #[clap(short, long)]
    prompts: Vec<String>,
    #[clap(short = 'o', long, default_value = "anthropic/claude-3.5-sonnet")]
    model: String,
    #[clap(short = 'n', long, default_value = "5")]
    max_concurrent: usize,
}

#[derive(Serialize, Deserialize, Debug)]
struct Message {
    role: String,
    content: String,
}

#[derive(Serialize, Debug)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<Message>,
}

#[derive(Deserialize, Debug)]
struct ChatCompletionResponse {
    id: String,
    model: String,
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Deserialize, Debug)]
struct Choice {
    message: Message,
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
    async fn create_chat_completion(&self, request: &ChatCompletionRequest) -> Result<ChatCompletionResponse> {
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

        if !response.status().is_success() {
            anyhow::bail!("OpenRouter API error: Status {}", response.status());
        }

        response.json::<ChatCompletionResponse>().await.context("Failed to parse response")
    }
}

fn load_config() -> Result<(String, String, String)> {
    dotenv().ok();
    Ok((
        env::var("OPENROUTER_API_KEY").context("OPENROUTER_API_KEY must be set")?,
        env::var("YOUR_SITE_URL").context("YOUR_SITE_URL must be set")?,
        env::var("YOUR_SITE_NAME").context("YOUR_SITE_NAME must be set")?,
    ))
}

async fn process_prompt(ai: &OpenAI, prompt: String, model: String) -> Result<()> {
    let request = ChatCompletionRequest {
        model,
        messages: vec![Message { role: "user".to_string(), content: prompt.clone() }],
    };

    match ai.create_chat_completion(&request).await {
        Ok(response) => {
            if let Some(choice) = response.choices.first() {
                println!("{}", "Prompt:".yellow().bold());
                println!("{}\n", prompt.yellow());
                println!("{}", "Response:".green().bold());
                println!("{}\n", choice.message.content.green());
                println!("{}", "Metadata:".cyan().bold());
                println!("Model: {}", response.model.cyan());
                println!("Tokens: {} prompt, {} completion, {} total",
                         response.usage.prompt_tokens.to_string().cyan(),
                         response.usage.completion_tokens.to_string().cyan(),
                         response.usage.total_tokens.to_string().cyan());
                println!("Response ID: {}", response.id.cyan());
                println!("{}", "-------------------------------------------------------------".cyan());
                info!("AI response received successfully.");
            }
            Ok(())
        }
        Err(e) => {
            error!("AI Error: {}", e);
            Err(e)
        }
    }
}

async fn repl(ai: Arc<OpenAI>, model: String, max_concurrent: usize) -> Result<()> {
    let mut rl = DefaultEditor::new()?;
    let semaphore = Arc::new(Semaphore::new(max_concurrent));

    println!("{}", "Interactive AI Assistant (type 'exit' to quit)".blue().bold());
    println!("{}", "-------------------------------------------------------------".blue());

    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::default_spinner()
        .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ")
        .template("{spinner} Processing...").unwrap());

    loop {
        let readline = rl.readline("You: ");
        match readline {
            Ok(line) => {
                if line.trim().eq_ignore_ascii_case("exit") {
                    break;
                }
                let _ = rl.add_history_entry(line.as_str());
                let ai = ai.clone();
                let semaphore = semaphore.clone();
                let model = model.clone();

                pb.enable_steady_tick(std::time::Duration::from_millis(100));

                match tokio::spawn(async move {
                    let _permit = semaphore.acquire().await.unwrap();
                    process_prompt(&ai, line, model).await
                }).await {
                    Ok(result) => {
                        pb.finish_and_clear();
                        if let Err(e) = result {
                            eprintln!("Error: {}", e);
                        }
                    }
                    Err(e) => {
                        pb.finish_and_clear();
                        eprintln!("Task panicked: {}", e);
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
                break;
            }
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                break;
            }
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
    }

    println!("{}", "Exiting AI Assistant.".blue().bold());
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let (api_key, site_url, site_name) = load_config()?;
    let opts: Opts = Opts::parse();

    let ai = Arc::new(OpenAI {
        client: Client::new(),
        api_key,
        base_url: "https://openrouter.ai/api/v1".to_string(),
        site_url,
        site_name,
    });

    if opts.prompts.is_empty() {
        repl(ai, opts.model, opts.max_concurrent).await?;
    } else {
        let semaphore = Arc::new(Semaphore::new(opts.max_concurrent));

        println!("{}", "Starting AI processing...".blue().bold());
        println!("{}", "-------------------------------------------------------------".blue());

        let tasks: Vec<_> = opts.prompts.into_iter().map(|prompt| {
            let ai = ai.clone();
            let semaphore = semaphore.clone();
            let model = opts.model.clone();
            tokio::spawn(async move {
                let _permit = semaphore.acquire().await.unwrap();
                process_prompt(&ai, prompt, model).await
            })
        }).collect();

        for task in tasks {
            task.await??;
        }

        println!("{}", "All prompts processed successfully.".blue().bold());
    }

    Ok(())
}
