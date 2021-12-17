use std::error::Error;

use futures_util::{future, TryStreamExt};
use kucoin::{client::SClient, spot::*};
use tokio::time;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let client = SClient::new();
    let ws_conn = client.ws().connect().await?;

    let topic = TopicKlines::new("BTC-USDT".parse()?, KlineInterval::OneMinute);
    let stream = ws_conn.subscribe(topic).await?;

    tokio::spawn(async move {
        time::sleep(time::Duration::from_secs(10)).await;

        println!("Closing the stream...");
        ws_conn.close().await;
    });

    println!("Printing 10 seconds of BTC klindes...");
    stream
        .try_for_each(|kline| {
            println!("{}", kline);
            future::ok(())
        })
        .await?;
    println!("Done!");

    Ok(())
}
