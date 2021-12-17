use std::error::Error;

use kucoin::{client::SClient, spot::*};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let client = SClient::new();
    let btc = "BTC-USDT".parse()?;
    let ticker = client.market().ticker(&btc).await?;
    println!("{}", ticker);

    Ok(())
}
