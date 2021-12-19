use std::env;
use std::error::Error;

use kucoin::{
    client::{Credentials, SClient},
    spot::*,
};
use tokio::time;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("KC_API_KEY")?;
    let api_pass = env::var("KC_API_PASS")?;
    let api_secret = env::var("KC_API_SECRET")?;
    let creds = Credentials::new(api_key, api_pass, api_secret);

    let client = SClient::with_credentials(creds);
    let ws_conn = client.ws().connect().await?;
    let live_order_book = ws_conn.live_order_book(&"BTC-USDT".parse()?).await?;

    tokio::spawn(async move {
        time::sleep(time::Duration::from_secs(10)).await;

        println!("Closing the stream...");
        ws_conn.close().await;
    });

    println!("Printing 10 seconds of BTC's best 10 live order book...");
    while live_order_book.still_alive().await {
        live_order_book
            .read(|book| {
                let mut s = String::new();
                s += "Asks\n--------------------\n";
                for (&price, &size) in book.asks.iter().take(5).rev() {
                    s += &format!("{:>8} | {:>8}\n", price, size);
                }
                s += "\nBids\n--------------------\n";
                for (&price, &size) in book.bids.iter().rev().take(5) {
                    s += &format!("{:>8} | {:>8}\n", price, size);
                }
                println!("{}", s);
            })
            .await;
        time::sleep(time::Duration::from_millis(250)).await;
    }
    println!("Done!");

    Ok(())
}
