// src/main.rs
use anyhow::{Context, Result};
use clap::Parser;
use colored::*;
use dotenv::dotenv;
use log::{error, info};
use reqwest::Client;
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
    choices: Vec<Choice>,
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
        messages: vec![Message { role: "user".to_string(), content: prompt }],
    };

    match ai.create_chat_completion(&request).await {
        Ok(response) => {
            if let Some(choice) = response.choices.first() {
                println!("{}", choice.message.content.green());
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

    let semaphore = Arc::new(Semaphore::new(opts.max_concurrent));

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

    Ok(())
}
