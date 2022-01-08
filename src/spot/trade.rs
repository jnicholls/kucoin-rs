use std::cmp;
use std::hash;

use async_trait::async_trait;
use derive_more::{Deref, Display};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    client::SClient,
    error::Error,
    spot::market::Symbol,
    time::{ts_nanoseconds, Time},
};

#[async_trait]
pub trait TradeApi {
    async fn place_order(&self, req: &OrderRequest) -> Result<OrderId, Error>;
}

struct Trade_(SClient);

impl SClient {
    pub fn trade(&self) -> impl TradeApi {
        Trade_(self.clone())
    }
}

#[async_trait]
impl TradeApi for Trade_ {
    async fn place_order(&self, req: &OrderRequest) -> Result<OrderId, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            order_id: OrderId,
        }

        self.0
            .post::<_, Response>("/api/v1/orders", req)
            .await
            .map(|r| r.order_id)
    }
}

#[derive(
    Clone, Debug, Deref, Deserialize, Display, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct OrderId(String);

impl OrderId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderRequest {
    #[serde(rename = "clientOid")]
    client_order_id: String,
    side: TradeSide,
    symbol: Symbol,
    #[serde(flatten)]
    order_type: OrderTypeRequest,
    remark: Option<String>,
    stp: SelfTradePrevention,
}

#[derive(Clone, Copy, Debug, Deserialize, Display, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OrderType {
    #[display(fmt = "limit")]
    Limit,
    #[display(fmt = "market")]
    Market,
}

impl Default for OrderType {
    fn default() -> Self {
        Self::Limit
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum OrderTypeRequest {
    Limit(OrderTypeLimit),
    Market(OrderTypeMarket),
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderTypeLimit {
    price: Decimal,
    size: Decimal,
    #[serde(default, flatten)]
    time_in_force: TimeInForce,
    #[serde(default, flatten)]
    visibility: OrderVisibility,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderTypeMarket {
    size: Option<Decimal>,
    funds: Option<Decimal>,
}

#[derive(Clone, Copy, Debug, Deserialize, Display, Serialize)]
#[serde(untagged)]
pub enum OrderVisibility {
    #[display(fmt = "visible")]
    Visible,
    #[display(fmt = "hidden = {}", hidden)]
    Hidden { hidden: bool },
    #[display(fmt = "iceberg = {}, visible_size = {}", iceberg, visible_size)]
    #[serde(rename_all = "camelCase")]
    Iceberg {
        iceberg: bool,
        visible_size: Decimal,
    },
}

impl Default for OrderVisibility {
    fn default() -> Self {
        Self::Visible
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Display, Serialize)]
pub enum SelfTradePrevention {
    #[display(fmt = "DC")]
    #[serde(rename = "DC")]
    DecreaseAndCancel,
    #[display(fmt = "CO")]
    #[serde(rename = "CO")]
    CancelOldest,
    #[display(fmt = "CN")]
    #[serde(rename = "CN")]
    CancelNewest,
    #[display(fmt = "CB")]
    #[serde(rename = "CB")]
    CancelBoth,
}

impl Default for SelfTradePrevention {
    fn default() -> Self {
        Self::CancelNewest
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Display, Serialize)]
#[serde(tag = "timeInForce")]
pub enum TimeInForce {
    #[display(fmt = "GTC")]
    #[serde(rename = "GTC", rename_all = "camelCase")]
    GoodTilCanceled {
        #[serde(default)]
        post_only: bool,
    },
    #[display(fmt = "GTT({})", cancel_after)]
    #[serde(rename = "GTT", rename_all = "camelCase")]
    GoodTilTime {
        cancel_after: u64,
        #[serde(default)]
        post_only: bool,
    },
    #[display(fmt = "IOC")]
    #[serde(rename = "IOC")]
    ImmediateOrCancel,
    #[display(fmt = "FOK")]
    #[serde(rename = "FOK")]
    FillOrKill,
}

impl Default for TimeInForce {
    fn default() -> Self {
        Self::GoodTilCanceled { post_only: true }
    }
}

#[derive(Clone, Debug, Deserialize, Display, Serialize)]
#[display(fmt = "{}: {} {} @ {}", time, side, size, price)]
#[serde(rename_all = "camelCase")]
pub struct Trade {
    pub sequence: Decimal,
    #[serde(with = "ts_nanoseconds")]
    pub time: Time,
    pub price: Decimal,
    pub size: Decimal,
    pub side: TradeSide,
}

impl Eq for Trade {}

impl PartialEq for Trade {
    fn eq(&self, other: &Self) -> bool {
        self.sequence == other.sequence && self.time == other.time
    }
}

impl Ord for Trade {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        match self.sequence.cmp(&other.sequence) {
            cmp::Ordering::Equal => self.time.cmp(&other.time),
            ordering => ordering,
        }
    }
}

impl PartialOrd for Trade {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl hash::Hash for Trade {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.sequence.hash(state);
        self.time.hash(state);
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Display, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "camelCase")]
pub enum TradeSide {
    #[display(fmt = "buy")]
    Buy,
    #[display(fmt = "sell")]
    Sell,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Display, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TradeType {
    #[display(fmt = "TRADE")]
    Trade,
    #[display(fmt = "MARGIN_TRADE")]
    #[serde(rename = "MARGIN_TRADE")]
    Margin,
}
