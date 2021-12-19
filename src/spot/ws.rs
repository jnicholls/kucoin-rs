use std::collections::{BTreeMap, HashMap};
use std::ops::Bound;
use std::str::FromStr;
use std::sync::{atomic, Arc};
use std::time::Duration;

use async_trait::async_trait;
use async_tungstenite::{
    tokio::{connect_async, ConnectStream},
    tungstenite::Message as WsMessage,
    WebSocketStream,
};
use derive_more::Display;
use futures_util::{
    sink::SinkExt,
    stream::{self, Stream, StreamExt},
};
use rust_decimal::Decimal;
use serde::{de, Deserialize, Serialize};
use snafu::{OptionExt, ResultExt};
use tokio::{
    sync::{mpsc, oneshot, Mutex, RwLock},
    task::JoinHandle,
    time,
};

use crate::{
    client::SClient,
    error::*,
    spot::*,
    time::{ts_nanoseconds_str, Time},
    utils::is_false,
};

#[async_trait]
pub trait WsApi {
    async fn connect(&self) -> Result<Connection, Error>;
}

struct Ws(SClient);

impl SClient {
    pub fn ws(&self) -> impl WsApi {
        Ws(self.clone())
    }
}

#[async_trait]
impl WsApi for Ws {
    async fn connect(&self) -> Result<Connection, Error> {
        // First we load up the connection information so we know where to connect and how to
        // authenticate.
        let info_url = match self.0.has_credentials() {
            false => "/api/v1/bullet-public",
            true => "/api/v1/bullet-private",
        };
        let conn_info = self.0.post::<_, ConnectInfo>(info_url, ()).await?;
        let token = conn_info.token;

        // Then we pick out the first encrypted websocket endpoint from the list. Usually there is
        // only one option, but we'll be prepared for when there are more than one anyways.
        let server = conn_info
            .servers
            .into_iter()
            .find(|s| matches!(s.protocol, ConnectProtocol::WebSocket) && s.encrypt)
            .context(WebsocketUnavailableSnafu)?;

        let url = format!("{}?token={}", server.endpoint, token);
        let (ws_stream, _) = connect_async(url).await.context(WebsocketSnafu)?;

        let (req_tx, req_rx) = mpsc::channel(1);
        let client = self.0.clone();
        let closed = Arc::new(Mutex::new(false));

        let inflight_reqs = HashMap::new();
        let ping_interval = server.ping_interval;
        let subscriptions = HashMap::new();
        let sub_mgr = SubscriptionManager {
            closed: closed.clone(),
            inflight_reqs,
            ping_interval,
            req_rx,
            subscriptions,
            ws_stream,
        };

        let hnd = tokio::spawn(sub_mgr.run());

        Ok(Connection {
            client,
            closed,
            hnd,
            req_tx,
        })
    }
}

#[derive(Debug)]
pub struct Connection {
    client: SClient,
    closed: Arc<Mutex<bool>>,
    hnd: JoinHandle<()>,
    req_tx: mpsc::Sender<SubscriptionRequest>,
}

impl Connection {
    pub async fn close(self) {
        let _ = self.req_tx.send(SubscriptionRequest::Close).await;
        let _ = self.hnd.await;
    }

    pub async fn is_closed(&self) -> bool {
        *self.closed.lock().await
    }

    pub async fn live_order_book(&self, symbol: &Symbol) -> Result<LiveOrderBook, Error> {
        let stream = self.subscribe(TopicLevel2::new(symbol.clone())).await?;
        let order_book = self
            .client
            .market()
            .order_book(symbol, OrderBookDepth::Full)
            .await?;

        Ok(LiveOrderBook::new(order_book, stream))
    }

    pub async fn subscribe<T: TopicToData>(
        &self,
        topic: T,
    ) -> Result<impl Stream<Item = Result<T::Output, Error>>, Error> {
        snafu::ensure!(!self.is_closed().await, WebsocketClosedSnafu);

        let req = Request::subscribe(topic.into());
        let (tx, rx) = oneshot::channel();

        self.req_tx
            .send(SubscriptionRequest::Subscribe(tx, req))
            .await
            .map_err(|_| WebsocketClosedSnafu.build())?;

        let mut recv = rx.await.map_err(|_| WebsocketClosedSnafu.build())??;
        let stream = stream::poll_fn(move |cx| recv.poll_recv(cx)).map(|v| {
            T::Data::deserialize(v)
                .map(Into::into)
                .context(WebsocketMessageDecodingSnafu)
        });

        Ok(stream)
    }

    pub async fn unsubscribe<T: TopicToData>(&self, topic: T) -> Result<(), Error> {
        snafu::ensure!(!self.is_closed().await, WebsocketClosedSnafu);

        let req = Request::unsubscribe(topic.into());
        let (tx, rx) = oneshot::channel();

        self.req_tx
            .send(SubscriptionRequest::Unsubscribe(tx, req))
            .await
            .map_err(|_| WebsocketClosedSnafu.build())?;

        rx.await.map_err(|_| WebsocketClosedSnafu.build())?
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Level3Trade {
    #[serde(with = "ts_nanoseconds_str")]
    pub time: Time,
    pub sequence: Decimal,
    pub symbol: Symbol,
    pub side: TradeSide,
    pub price: Decimal,
    pub size: Decimal,
    pub trade_id: String,
    pub taker_order_id: String,
    pub maker_order_id: String,
}

#[derive(Clone, Debug)]
pub struct LiveOrderBook {
    inner: Arc<RwLock<LiveOrderBookInner>>,
}

#[derive(Debug)]
struct LiveOrderBookInner {
    order_book: OrderBook,
    still_alive: bool,
}

impl LiveOrderBook {
    fn new<S>(order_book: OrderBook, mut stream: S) -> Self
    where
        S: Stream<Item = Result<BTreeMap<Decimal, (bool, Decimal, Decimal)>, Error>>
            + Send
            + Unpin
            + 'static,
    {
        let inner = Arc::new(RwLock::new(LiveOrderBookInner {
            order_book,
            still_alive: true,
        }));
        let live_order_book = Self {
            inner: inner.clone(),
        };

        tokio::spawn(async move {
            let mut next_book = inner.read().await.order_book.clone();
            let mut last_seq = next_book.sequence;
            let mut sync_book = time::interval(Duration::from_millis(500));
            sync_book.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    biased;

                    _ = sync_book.tick() => {
                        if last_seq < next_book.sequence {
                            last_seq = next_book.sequence;
                            inner.write().await.order_book = next_book.clone();
                        }
                    },
                    result = stream.next() => match result {
                        Some(Ok(updates)) => {
                            // Update the next_book according to the Level 2 sequencing rules.
                            // First rule, we want to start by only looking at sequence updates that
                            // are greater than the next book's current sequence number.
                            for (&seq, &(is_bid, price, size)) in updates.range((Bound::Excluded(next_book.sequence), Bound::Unbounded)) {
                                // For each sequence update, the rules are as follows:
                                //   1. If the price is 0, ignore it.
                                //   2. If the size is 0, remove it from the book.
                                //   3. If neither 1 or 2, set the new size for the price.
                                if price > Decimal::ZERO {
                                    next_book.bids.remove(&price).or_else(|| next_book.asks.remove(&price));
                                    if size > Decimal::ZERO {
                                        if is_bid {
                                            next_book.bids.insert(price, size);
                                        } else {
                                            next_book.asks.insert(price, size);
                                        }
                                    }
                                }

                                next_book.sequence = seq;
                            }
                        }
                        Some(Err(_)) | None => {
                            inner.write().await.still_alive = false;
                            break;
                        }
                    },
                }
            }
        });

        live_order_book
    }

    pub async fn read<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&OrderBook) -> R,
    {
        f(&self.inner.read().await.order_book)
    }

    pub async fn still_alive(&self) -> bool {
        self.inner.read().await.still_alive
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolSnapshot {
    #[serde(rename = "datetime")]
    pub time: Time,
    pub trading: bool,
    pub symbol: Symbol,
    pub buy: Decimal,
    pub sell: Decimal,
    pub change_price: Decimal,
    pub change_rate: Decimal,
    pub sort: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    #[serde(rename = "vol")]
    pub vol_base: Decimal,
    #[serde(rename = "volValue")]
    pub vol_quote: Decimal,
    pub last_traded_price: Decimal,
    pub board: Decimal,
    pub mark: Decimal,
}

#[derive(Clone, Debug, Deserialize, Display, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(into = "String", try_from = "String")]
pub enum Topic {
    #[display(fmt = "/account/balance")]
    AccountBalance,

    #[display(fmt = "/spotMarket/advancedOrders")]
    AdvancedOrder,

    #[display(fmt = "/market/candles:{}_{}", symbol, interval)]
    Klines {
        symbol: Symbol,
        interval: KlineInterval,
    },

    #[display(fmt = "/market/level2:{}", _0)]
    Level2(Symbol),

    #[display(fmt = "/spotMarket/level2Depth5:{}", _0)]
    Level2Best5(Symbol),

    #[display(fmt = "/spotMarket/level2Depth50:{}", _0)]
    Level2Best50(Symbol),

    #[display(fmt = "/spotMarket/tradeOrders")]
    OrderChange,

    #[display(fmt = "/market/snapshot:{}", _0)]
    Snapshot(Symbol),

    #[display(fmt = "/market/ticker:{}", _0)]
    Ticker(Symbol),

    #[display(fmt = "/market/match:{}", _0)]
    Trade(Symbol),
}

impl Topic {
    fn is_private(&self) -> bool {
        use Topic::*;

        match self {
            AccountBalance | AdvancedOrder | OrderChange => true,
            Klines { .. }
            | Level2(_)
            | Level2Best5(_)
            | Level2Best50(_)
            | Snapshot(_)
            | Ticker(_)
            | Trade(_) => false,
        }
    }
}

impl From<Topic> for String {
    fn from(t: Topic) -> Self {
        t.to_string()
    }
}

impl FromStr for Topic {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use Topic::*;

        // A topic has a name, and some paramters usually. We'll first figure out which topic we're
        // dealing with, and then we'll take the remaining part of the topic and parse the
        // parameters.
        let mut parts = s.splitn(2, ':');
        parts
            .next()
            .zip(parts.next())
            .map(|(topic_str, params)| {
                Ok(match topic_str {
                    "/account/balance" => AccountBalance,
                    "/spotMarket/advancedOrders" => AdvancedOrder,
                    "/market/candles" => {
                        let mut parts = params.splitn(2, '_');
                        parts
                            .next()
                            .zip(parts.next())
                            .map(|(symbol_str, interval_str)| {
                                Ok(Klines {
                                    symbol: symbol_str.parse()?,
                                    interval: interval_str.parse()?,
                                })
                            })
                            .transpose()?
                            .ok_or_else(|| {
                                ParseSnafu {
                                    reason: format!(
                                        "The klines symbol_interval combination '{}' is not underscore-delimited",
                                        params
                                    ),
                                }
                                .build()
                            })?
                    }
                    "/market/level2" => Level2(params.parse()?),
                    "/spotMarket/level2Depth5" => Level2Best5(params.parse()?),
                    "/spotMarket/level2Depth50" => Level2Best50(params.parse()?),
                    "/spotMarket/tradeOrders" => OrderChange,
                    "/market/snapshot" => Snapshot(params.parse()?),
                    "/market/ticker" => Ticker(params.parse()?),
                    "/market/match" => Trade(params.parse()?),
                    _ => ParseSnafu {
                        reason: format!("The topic '{}' is not recognized", topic_str),
                    }
                    .fail()?,
                })
            })
            .transpose()?
            .ok_or_else(|| {
                ParseSnafu {
                    reason: format!("The topic '{}' is not colon-delimited", s),
                }
                .build()
            })
    }
}

impl TryFrom<String> for Topic {
    type Error = <Topic as FromStr>::Err;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

pub trait TopicToData: Into<Topic> {
    type Data: de::DeserializeOwned;
    type Output: From<Self::Data>;
}

#[derive(Clone, Debug)]
pub struct TopicAccountBalance(Topic);

impl TopicAccountBalance {
    pub fn new() -> Self {
        Self(Topic::AccountBalance)
    }
}

impl From<TopicAccountBalance> for Topic {
    fn from(t: TopicAccountBalance) -> Self {
        t.0
    }
}

#[derive(Clone, Debug)]
pub struct TopicAdvancedOrder(Topic);

impl TopicAdvancedOrder {
    pub fn new() -> Self {
        Self(Topic::AdvancedOrder)
    }
}

impl From<TopicAdvancedOrder> for Topic {
    fn from(t: TopicAdvancedOrder) -> Self {
        t.0
    }
}

#[derive(Clone, Debug)]
pub struct TopicKlines(Topic);

impl TopicKlines {
    pub fn new(symbol: Symbol, interval: KlineInterval) -> Self {
        Self(Topic::Klines { symbol, interval })
    }
}

impl From<TopicKlines> for Topic {
    fn from(t: TopicKlines) -> Self {
        t.0
    }
}

impl TopicToData for TopicKlines {
    type Data = WsKline;
    type Output = Kline;
}

#[derive(Clone, Debug)]
pub struct TopicLevel2Best5(Topic);

impl TopicLevel2Best5 {
    pub fn new(symbol: Symbol) -> Self {
        Self(Topic::Level2Best5(symbol))
    }
}

impl From<TopicLevel2Best5> for Topic {
    fn from(t: TopicLevel2Best5) -> Self {
        t.0
    }
}

impl TopicToData for TopicLevel2Best5 {
    type Data = OrderBook;
    type Output = Self::Data;
}

#[derive(Clone, Debug)]
pub struct TopicLevel2Best50(Topic);

impl TopicLevel2Best50 {
    pub fn new(symbol: Symbol) -> Self {
        Self(Topic::Level2Best50(symbol))
    }
}

impl From<TopicLevel2Best50> for Topic {
    fn from(t: TopicLevel2Best50) -> Self {
        t.0
    }
}

impl TopicToData for TopicLevel2Best50 {
    type Data = OrderBook;
    type Output = Self::Data;
}

#[derive(Clone, Debug)]
pub struct TopicOrderChange(Topic);

impl TopicOrderChange {
    pub fn new() -> Self {
        Self(Topic::OrderChange)
    }
}

impl From<TopicOrderChange> for Topic {
    fn from(t: TopicOrderChange) -> Self {
        t.0
    }
}

#[derive(Clone, Debug)]
pub struct TopicSnapshot(Topic);

impl TopicSnapshot {
    pub fn new(symbol: Symbol) -> Self {
        Self(Topic::Snapshot(symbol))
    }
}

impl From<TopicSnapshot> for Topic {
    fn from(t: TopicSnapshot) -> Self {
        t.0
    }
}

impl TopicToData for TopicSnapshot {
    type Data = WsSymbolSnapshot;
    type Output = SymbolSnapshot;
}

#[derive(Clone, Debug)]
pub struct TopicTicker(Topic);

impl TopicTicker {
    pub fn new(symbol: Symbol) -> Self {
        Self(Topic::Ticker(symbol))
    }
}

impl From<TopicTicker> for Topic {
    fn from(t: TopicTicker) -> Self {
        t.0
    }
}

impl TopicToData for TopicTicker {
    type Data = Ticker;
    type Output = Self::Data;
}

#[derive(Clone, Debug)]
pub struct TopicTrade(Topic);

impl TopicTrade {
    pub fn new(symbol: Symbol) -> Self {
        Self(Topic::Trade(symbol))
    }
}

impl From<TopicTrade> for Topic {
    fn from(t: TopicTrade) -> Self {
        t.0
    }
}

impl TopicToData for TopicTrade {
    type Data = Level3Trade;
    type Output = Self::Data;
}

#[derive(Deserialize)]
pub struct WsKline {
    candles: Kline,
}

impl From<WsKline> for Kline {
    fn from(v: WsKline) -> Self {
        v.candles
    }
}

#[derive(Deserialize)]
pub struct WsSymbolSnapshot {
    data: SymbolSnapshot,
}

impl From<WsSymbolSnapshot> for SymbolSnapshot {
    fn from(v: WsSymbolSnapshot) -> Self {
        v.data
    }
}

///////////////////////////////////////////////////////////////////////////////

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectInfo {
    #[serde(rename = "instanceServers")]
    servers: Vec<ConnectServer>,
    token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectServer {
    endpoint: String,
    protocol: ConnectProtocol,
    encrypt: bool,
    ping_interval: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum ConnectProtocol {
    WebSocket,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase", tag = "type")]
enum Event {
    Ack {
        id: String,
    },
    Error {
        id: String,
        code: SystemCode,
        data: String,
    },
    Message {
        topic: Topic,
        data: serde_value::Value,
    },
    Pong,
    Welcome,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Level2Update {
    changes: Level2Changes,
}

#[derive(Deserialize)]
struct Level2Changes {
    asks: Vec<(Decimal, Decimal, Decimal)>,
    bids: Vec<(Decimal, Decimal, Decimal)>,
}

impl From<Level2Update> for BTreeMap<Decimal, (bool, Decimal, Decimal)> {
    fn from(v: Level2Update) -> Self {
        let mut set = BTreeMap::new();

        for (price, size, seq) in v.changes.asks {
            set.insert(seq, (false, price, size));
        }

        for (price, size, seq) in v.changes.bids {
            set.insert(seq, (true, price, size));
        }

        set
    }
}

static REQUEST_ID: atomic::AtomicU64 = atomic::AtomicU64::new(1);

fn next_req_id() -> String {
    REQUEST_ID
        .fetch_add(1, atomic::Ordering::Relaxed)
        .to_string()
}

#[derive(Serialize)]
struct Request {
    id: String,
    #[serde(flatten)]
    req_type: RequestType,
    #[serde(rename = "privateChannel", skip_serializing_if = "is_false")]
    private_channel: bool,
    #[serde(skip_serializing_if = "is_false")]
    response: bool,
}

impl Request {
    fn subscribe(topic: Topic) -> Self {
        let private_channel = topic.is_private();

        Self {
            id: next_req_id(),
            req_type: RequestType::Subscribe { topic },
            private_channel,
            response: true,
        }
    }

    fn unsubscribe(topic: Topic) -> Self {
        Self {
            id: next_req_id(),
            req_type: RequestType::Unsubscribe { topic },
            private_channel: false,
            response: true,
        }
    }

    fn ping() -> Self {
        Self {
            id: next_req_id(),
            req_type: RequestType::Ping,
            private_channel: false,
            response: false,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase", tag = "type")]
enum RequestType {
    Subscribe { topic: Topic },
    Unsubscribe { topic: Topic },
    Ping,
}

struct SubscriptionManager {
    closed: Arc<Mutex<bool>>,
    inflight_reqs: HashMap<String, SubscriptionRequest>,
    ping_interval: u64,
    req_rx: mpsc::Receiver<SubscriptionRequest>,
    subscriptions: HashMap<Topic, mpsc::Sender<serde_value::Value>>,
    ws_stream: WebSocketStream<ConnectStream>,
}

impl SubscriptionManager {
    async fn run(mut self) {
        let mut ping = time::interval(Duration::from_secs(self.ping_interval));
        ping.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        loop {
            let result = tokio::select! {
                biased;

                _ = ping.tick() => self.send_ping().await,
                sub_req = self.req_rx.recv() => match sub_req {
                    Some(sub_req) => self.handle_req(sub_req).await,
                    None => WebsocketClosedSnafu.fail(),
                },
                msg = self.ws_stream.next() => match msg {
                    Some(Ok(msg)) => self.handle_msg(msg).await,
                    Some(Err(e)) => Err(e).context(WebsocketSnafu),
                    None => WebsocketClosedSnafu.fail(),
                },
            };

            if let Err(_) = result {
                *self.closed.lock().await = true;
                let _ = self.ws_stream.close(None).await;
                break;
            }
        }
    }

    async fn handle_msg(&mut self, msg: WsMessage) -> Result<(), Error> {
        let event: Event = match msg {
            WsMessage::Binary(json) => {
                serde_json::from_slice(&json).with_context(|_| ResponseDecodingSnafu {
                    resp_json: String::from_utf8_lossy(&json),
                })?
            }
            WsMessage::Text(json) => {
                serde_json::from_str(&json).context(ResponseDecodingSnafu { resp_json: json })?
            }
            WsMessage::Close(_) => return WebsocketClosedSnafu.fail(),
            _ => return Ok(()),
        };

        match event {
            Event::Ack { id } => {
                if let Some(sub_req) = self.inflight_reqs.remove(&id) {
                    match sub_req {
                        SubscriptionRequest::Subscribe(tx, req) => {
                            if let RequestType::Subscribe { topic } = req.req_type {
                                let (stream_tx, stream_rx) = mpsc::channel(64);

                                if let Ok(_) = tx.send(Ok(stream_rx)) {
                                    self.subscriptions.insert(topic, stream_tx);
                                }
                            }
                        }

                        SubscriptionRequest::Unsubscribe(tx, req) => {
                            if let RequestType::Unsubscribe { topic } = req.req_type {
                                self.subscriptions.remove(&topic);
                                let _ = tx.send(Ok(()));
                            }
                        }

                        SubscriptionRequest::Close => (),
                    }
                }
            }
            Event::Error {
                id,
                code,
                data: msg,
            } => {
                if let Some(sub_req) = self.inflight_reqs.remove(&id) {
                    match sub_req {
                        SubscriptionRequest::Subscribe(tx, _) => {
                            let _ =
                                tx.send(Err(SystemError { code, msg }).context(BadRequestSnafu));
                        }

                        SubscriptionRequest::Unsubscribe(tx, _) => {
                            let _ =
                                tx.send(Err(SystemError { code, msg }).context(BadRequestSnafu));
                        }

                        SubscriptionRequest::Close => (),
                    }
                }
            }
            Event::Message { topic, data } => {
                if let Some(tx) = self.subscriptions.get(&topic) {
                    if let Err(_) = tx.send(data).await {
                        self.subscriptions.remove(&topic);
                    }
                }
            }
            _ => (),
        }

        Ok(())
    }

    async fn handle_req(&mut self, sub_req: SubscriptionRequest) -> Result<(), Error> {
        let req = match &sub_req {
            SubscriptionRequest::Subscribe(_, req) => req,
            SubscriptionRequest::Unsubscribe(_, req) => req,
            SubscriptionRequest::Close => return WebsocketClosedSnafu.fail(),
        };

        self.send_data(req).await?;
        self.inflight_reqs.insert(req.id.clone(), sub_req);
        Ok(())
    }

    async fn send_data<T>(&mut self, msg: T) -> Result<(), Error>
    where
        T: Serialize,
    {
        let json = serde_json::to_string(&msg).context(JsonEncodingSnafu)?;
        self.ws_stream
            .send(WsMessage::Text(json))
            .await
            .context(WebsocketSnafu)
    }

    async fn send_ping(&mut self) -> Result<(), Error> {
        self.send_data(Request::ping()).await
    }
}

enum SubscriptionRequest {
    Subscribe(
        oneshot::Sender<Result<mpsc::Receiver<serde_value::Value>, Error>>,
        Request,
    ),
    Unsubscribe(oneshot::Sender<Result<(), Error>>, Request),
    Close,
}

struct TopicLevel2(Topic);

impl TopicLevel2 {
    fn new(symbol: Symbol) -> Self {
        Self(Topic::Level2(symbol))
    }
}

impl From<TopicLevel2> for Topic {
    fn from(t: TopicLevel2) -> Self {
        t.0
    }
}

impl TopicToData for TopicLevel2 {
    type Data = Level2Update;
    type Output = BTreeMap<Decimal, (bool, Decimal, Decimal)>;
}
