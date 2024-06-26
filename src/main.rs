#![allow(non_snake_case)]
// /path/to/your/project/src/main.rs

use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::json;
use std::env;
use std::fs;
use std::path::Path;
use dotenv::dotenv;

const API_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

struct OpenRouterClient {
    client: reqwest::Client,
    api_key: String,
}

impl OpenRouterClient {
    fn new(api_key: String) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "HTTP-Referer",
            HeaderValue::from_static("https://asdf.asdf"),
        );
        headers.insert("X-Title", HeaderValue::from_static("asdf"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self { client, api_key })
    }

    async fn proofread(&self, content: &str) -> Result<String> {
        let auth_header = format!("Bearer {}", self.api_key);
        let response = self
            .client
            .post(API_URL)
            .header(AUTHORIZATION, auth_header)
            .json(&json!({
                "model": "google/gemini-pro-1.5",
                "messages": [
                    {
                        "role": "system",
                        "content": "You are a professional proofreader. Correct any grammatical and spelling errors in the following text and return the corrected version. Do not remove jokes or change the writing style"
                    },
                    {
                        "role": "user",
                        "content": content
                    }
                ]
            }))
            .send()
            .await
            .context("Failed to send request to OpenRouter API")?;

        let result: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse API response")?;

        result["choices"][0]["message"]["content"]
            .as_str()
            .map(String::from)
            .context("Failed to extract content from API response")
    }
}

fn read_file(path: &Path) -> Result<String> {
    fs::read_to_string(path).context("Failed to read input file")
}

fn write_file(path: &Path, content: &str) -> Result<()> {
    fs::write(path, content).context("Failed to write output file")
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file
    dotenv().context("Failed to load .env file")?;

    let api_key = env::var("OPENROUTER_API_KEY").context("OPENROUTER_API_KEY not set in .env file")?;
    let input_path = Path::new("input.txt");
    let output_path = Path::new("output.txt");

    let client = OpenRouterClient::new(api_key)?;

    let input_text = read_file(input_path)?;
    let proofread_text = client.proofread(&input_text).await?;
    write_file(output_path, &proofread_text)?;

    println!("Proofreading complete. Result saved to {:?}", output_path);

    Ok(())
}
