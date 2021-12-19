# kucoin-rs
An async, strongly-typed Rust library for the KuCoin API.

This library is a personal project that I have decided to release publicly. There is a lot to do! *Please consider it __experimental__ at this time.*

*Note: While Spot, Margin, and Futures APIs are the goal for this API, currently the focus of development is on the Spot & Market markets*

## Todo
* [ ] Finish Spot APIs
* [ ] Futures API
* [ ] Documentation
* [ ] Publish on crates.io
* [ ] Unit tests with deterministic replay (e.g. something akin to Ruby's [VCR](https://github.com/vcr/vcr))
* [ ] GitHub Actions CI setup
* [ ] All the cool badges (link to docs, CI status, MSRV, etc.)
* [x] Open source some of my higher-level APIs (e.g. in-memory LiveOrderBook)
* [ ] Provide bridges to async-std and smog async runtimes

## Installation

Add to your crate a new dependency (_TODO: Update when published to crates.io_):

```toml
[dependencies]
kucoin = { git = "https://github.com/jnicholls/kucoin-rs", branch = "main" }
```

This crate depends on [Tokio](https://tokio.rs) and expects to operate in a Tokio runtime.

## Example Usage

Print out BTC current ticker information
```rust
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
```

Stream real-time BTC minutely klines for 10 seconds.
```rust
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
```
