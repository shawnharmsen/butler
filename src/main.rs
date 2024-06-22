use reqwest::Error;
use serde::{Deserialize};

#[derive(Deserialize, Debug)]
struct TimeApiResponse {
    datetime: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let url = "https://worldtimeapi.org/api/timezone/Etc/UTC";
    let response = reqwest::get(url).await?;
    let time_api_response: TimeApiResponse = response.json().await?;
    println!("The current time is: {}", time_api_response.datetime);
    Ok(())
}