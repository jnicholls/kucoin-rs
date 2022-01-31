use std::env;
use std::error::Error;

use futures_util::TryStreamExt;
use kucoin::{
    client::{Credentials, SClient},
    spot::*,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("KC_API_KEY")?;
    let api_pass = env::var("KC_API_PASS")?;
    let api_secret = env::var("KC_API_SECRET")?;
    let creds = Credentials::new(api_key, api_pass, api_secret);
    let client = SClient::with_credentials(creds);

    client
        .trade()
        .orders(&OrderFilter::new())
        .try_for_each(|orders| async move {
            println!("{:?}", orders);
            Ok(())
        })
        .await?;

    Ok(())
}
