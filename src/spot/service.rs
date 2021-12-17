use async_trait::async_trait;
use derive_more::Display;
use serde::{Deserialize, Serialize};

use crate::{client::SClient, error::Error, time::Time};

#[async_trait]
pub trait ServiceApi {
    async fn status(&self) -> Result<Status, Error>;

    async fn time(&self) -> Result<Time, Error>;
}

struct Service(SClient);

impl SClient {
    pub fn service(&self) -> impl ServiceApi {
        Service(self.clone())
    }
}

#[async_trait]
impl ServiceApi for Service {
    async fn status(&self) -> Result<Status, Error> {
        self.0.get("/api/v1/status", ()).await
    }

    async fn time(&self) -> Result<Time, Error> {
        self.0.get("/api/v1/timestamp", ()).await
    }
}

#[derive(Clone, Debug, Deserialize, Display, Serialize)]
#[serde(rename_all = "lowercase", tag = "status")]
pub enum Status {
    #[display(fmt = "OPEN")]
    Open,
    #[display(fmt = "CLOSE")]
    Close,
    #[display(fmt = "CANCEL ONLY")]
    CancelOnly,
}
