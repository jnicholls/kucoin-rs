use std::cmp;
use std::collections::BTreeMap;
use std::hash;

use async_trait::async_trait;
use derive_more::{Deref, Display};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    client::SClient,
    error::Error,
    pagination::PagedStream,
    spot::market::{CurrencyCode, Symbol},
    time::{ts_nanoseconds, Time},
};

#[async_trait]
pub trait TradeApi {
    async fn cancel_all_orders(
        &self,
        trade_type: TradeType,
        symbol: Option<&Symbol>,
    ) -> Result<Vec<OrderId>, Error>;
    async fn cancel_order_by_client_id(&self, client_id: &str) -> Result<bool, Error>;
    async fn cancel_order_by_id(&self, order_id: &OrderId) -> Result<bool, Error>;
    fn get_filled_orders<'a>(&'a self, filter: &'a OrderFilter) -> PagedStream<FilledOrder>;
    fn get_filled_orders_by_id<'a>(&'a self, order_id: &'a OrderId) -> PagedStream<FilledOrder>;
    async fn get_order_by_client_id(&self, client_id: &str) -> Result<Order, Error>;
    async fn get_order_by_id(&self, order_id: &OrderId) -> Result<Order, Error>;
    fn get_orders<'a>(&'a self, filter: &'a OrderFilter) -> PagedStream<Order>;
    async fn get_recent_filled_orders(&self) -> Result<Vec<FilledOrder>, Error>;
    async fn get_recent_orders(&self) -> Result<Vec<Order>, Error>;
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
    async fn cancel_all_orders(
        &self,
        trade_type: TradeType,
        symbol: Option<&Symbol>,
    ) -> Result<Vec<OrderId>, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            cancelled_order_ids: Vec<OrderId>,
        }

        let mut params = BTreeMap::new();
        params.insert("tradeType", trade_type.to_string());
        if let Some(symbol) = symbol {
            params.insert("symbol", symbol.to_string());
        }

        self.0
            .delete::<_, Response>("/api/v1/orders", params)
            .await
            .map(|r| r.cancelled_order_ids)
    }

    async fn cancel_order_by_client_id(&self, client_id: &str) -> Result<bool, Error> {
        self.0
            .delete::<_, ()>(&format!("/api/v1/order/client-order/{}", client_id), ())
            .await?;

        let order = self.get_order_by_client_id(client_id).await?;
        Ok(!order.is_active)
    }

    async fn cancel_order_by_id(&self, order_id: &OrderId) -> Result<bool, Error> {
        self.0
            .delete::<_, ()>(&format!("/api/v1/orders/{}", order_id), ())
            .await?;

        let order = self.get_order_by_id(order_id).await?;
        Ok(!order.is_active)
    }

    fn get_filled_orders<'a>(&'a self, filter: &'a OrderFilter) -> PagedStream<FilledOrder> {
        self.0.paged_get("/api/v1/fills", filter)
    }

    fn get_filled_orders_by_id<'a>(&'a self, order_id: &'a OrderId) -> PagedStream<FilledOrder> {
        self.0.paged_get("/api/v1/fills", [("orderId", order_id)])
    }

    async fn get_order_by_client_id(&self, client_id: &str) -> Result<Order, Error> {
        self.0
            .get(&format!("/api/v1/order/client-order/{}", client_id), ())
            .await
    }

    async fn get_order_by_id(&self, order_id: &OrderId) -> Result<Order, Error> {
        self.0
            .get(&format!("/api/v1/orders/{}", order_id), ())
            .await
    }

    fn get_orders<'a>(&'a self, filter: &'a OrderFilter) -> PagedStream<Order> {
        self.0.paged_get("/api/v1/orders", filter)
    }

    async fn get_recent_filled_orders(&self) -> Result<Vec<FilledOrder>, Error> {
        self.0.get("/api/v1/limit/fills", ()).await
    }

    async fn get_recent_orders(&self) -> Result<Vec<Order>, Error> {
        self.0.get("/api/v1/limit/orders", ()).await
    }

    async fn place_order(&self, req: &OrderRequest) -> Result<OrderId, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            order_id: OrderId,
        }

        let path = match req {
            OrderRequest { stop: Some(_), .. } => "/api/v1/stop-order",
            OrderRequest {
                trade_type: TradeType::Margin,
                ..
            } => "/api/v1/margin/order",
            _ => "/api/v1/orders",
        };

        self.0
            .post::<_, Response>(path, req)
            .await
            .map(|r| r.order_id)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilledOrder {
    pub symbol: Symbol,
    pub trade_id: TradeId,
    pub order_id: OrderId,
    pub counter_order_id: OrderId,
    pub side: TradeSide,
    pub price: Decimal,
    pub size: Decimal,
    pub funds: Decimal,
    #[serde(rename = "type")]
    pub order_type: OrderType,
    pub fee: Decimal,
    pub fee_currency: CurrencyCode,
    #[serde(flatten)]
    pub stop: StopOrder,
    pub liquidity: TradeLiquidity,
    pub force_taker: bool,
    pub created_at: Time,
    pub trade_type: TradeType,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    #[serde(alias = "orderId")]
    pub id: OrderId,
    pub symbol: Symbol,
    #[serde(rename = "type")]
    pub order_type: OrderType,
    pub side: TradeSide,
    pub price: Decimal,
    pub size: Decimal,
    pub funds: Decimal,
    pub deal_funds: Decimal,
    pub deal_size: Decimal,
    pub fee: Decimal,
    pub fee_currency: CurrencyCode,
    pub stp: SelfTradePrevention,
    #[serde(flatten)]
    pub stop: StopOrder,
    #[serde(flatten)]
    pub time_in_force: TimeInForce,
    #[serde(flatten)]
    pub visibility: OrderVisibility,
    #[serde(rename = "clientOid")]
    pub client_order_id: Option<String>,
    pub remark: Option<String>,
    pub is_active: bool,
    pub created_at: Time,
    pub trade_type: TradeType,
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

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbol: Option<Symbol>,
    #[serde(skip_serializing_if = "Option::is_none")]
    side: Option<TradeSide>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trade_type: Option<TradeType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_at: Option<Time>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_at: Option<Time>,
}

impl OrderFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn active_only(self) -> Self {
        let status = Some("active".to_string());
        Self { status, ..self }
    }

    pub fn done_only(self) -> Self {
        let status = Some("done".to_string());
        Self { status, ..self }
    }

    pub fn side(self, side: TradeSide) -> Self {
        let side = Some(side);
        Self { side, ..self }
    }

    pub fn trade_type(self, trade_type: TradeType) -> Self {
        let trade_type = Some(trade_type);
        Self { trade_type, ..self }
    }

    pub fn time(self, start_at: Time, end_at: Time) -> Self {
        let start_at = Some(start_at);
        let end_at = Some(end_at);
        Self {
            start_at,
            end_at,
            ..self
        }
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
    #[serde(flatten)]
    stop: Option<StopOrder>,
    stp: SelfTradePrevention,
    trade_type: TradeType,
    auto_borrow: bool,
}

impl OrderRequest {
    pub fn new(
        client_order_id: impl Into<String>,
        side: TradeSide,
        symbol: Symbol,
        order_type: OrderTypeRequest,
    ) -> Self {
        let client_order_id = client_order_id.into();
        let remark = None;
        let stop = None;
        let stp = Default::default();
        let trade_type = TradeType::Trade;
        let auto_borrow = true;

        Self {
            client_order_id,
            side,
            symbol,
            order_type,
            remark,
            stop,
            stp,
            trade_type,
            auto_borrow,
        }
    }

    pub fn margin(self) -> Self {
        let trade_type = TradeType::Margin;
        Self { trade_type, ..self }
    }

    pub fn remark(self, remark: impl Into<String>) -> Self {
        // Remarks can only be 100 characters long at most.
        let mut remark = remark.into();
        remark.truncate(100);

        let remark = Some(remark.into());
        Self { remark, ..self }
    }

    pub fn spot(self) -> Self {
        let trade_type = TradeType::Trade;
        Self { trade_type, ..self }
    }

    pub fn stop(self, stop: StopOrder) -> Self {
        let stop = Some(stop);
        Self { stop, ..self }
    }

    pub fn stp(self, stp: SelfTradePrevention) -> Self {
        Self { stp, ..self }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Display, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    #[display(fmt = "limit")]
    Limit,
    #[display(fmt = "market")]
    Market,
    #[display(fmt = "stop_limit")]
    #[serde(alias = "limit_stop", alias = "stop")]
    StopLimit,
    #[display(fmt = "stop_market")]
    #[serde(alias = "market_stop")]
    StopMarket,
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

impl OrderTypeLimit {
    pub fn new(price: Decimal, size: Decimal) -> Self {
        let time_in_force = Default::default();
        let visibility = Default::default();

        Self {
            price,
            size,
            time_in_force,
            visibility,
        }
    }

    pub fn time_in_force(self, time_in_force: TimeInForce) -> Self {
        Self {
            time_in_force,
            ..self
        }
    }

    pub fn visibility(self, visibility: OrderVisibility) -> Self {
        Self { visibility, ..self }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OrderTypeMarket {
    #[serde(rename = "size")]
    Base(Decimal),
    #[serde(rename = "funds")]
    Quote(Decimal),
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
    #[serde(alias = "", rename = "CN")]
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

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "stop")]
pub enum StopOrder {
    Entry {
        #[serde(default, rename = "stopTriggered")]
        triggered: bool,
        #[serde(default, rename = "stopPrice")]
        price: Decimal,
    },
    Loss {
        #[serde(default, rename = "stopTriggered")]
        triggered: bool,
        #[serde(default, rename = "stopPrice")]
        price: Decimal,
    },
    #[serde(other)]
    Unknown,
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
    Clone, Debug, Deref, Deserialize, Display, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct TradeId(String);

impl TradeId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Display, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "camelCase")]
pub enum TradeLiquidity {
    #[display(fmt = "maker")]
    Maker,
    #[display(fmt = "taker")]
    Taker,
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
