use std::borrow::Borrow;
use std::cmp;
use std::collections::{BTreeMap, HashMap};
use std::hash;
use std::str::FromStr;

use async_trait::async_trait;
use derive_more::{Deref, Display};
use rust_decimal::Decimal;
use serde::{de, Deserialize, Serialize};

use crate::{
    client::SClient,
    error::{Error, ParseSnafu},
    time::{ts_nanoseconds, ts_seconds, ts_seconds_str, Time},
    utils::iter_to_csv,
};

#[async_trait]
pub trait MarketApi {
    async fn currencies(&self) -> Result<Vec<Currency>, Error>;

    async fn currency(&self, code: &CurrencyCode) -> Result<Currency, Error>;

    async fn current_prices_usd(
        &self,
        req: &PricesRequest,
    ) -> Result<HashMap<CurrencyCode, Decimal>, Error>;

    async fn klines(&self, req: &KlinesRequest) -> Result<Vec<Kline>, Error>;

    async fn markets(&self) -> Result<Vec<Market>, Error>;

    async fn order_book(&self, symbol: &Symbol, depth: OrderBookDepth) -> Result<OrderBook, Error>;

    async fn stats_24hr(&self, req: &Stats24hrRequest) -> Result<Vec<Stats24hr>, Error>;

    async fn ticker(&self, symbol: &Symbol) -> Result<Ticker, Error>;

    async fn trades(&self, symbol: &Symbol) -> Result<Vec<Trade>, Error>;
}

struct Market_(SClient);

impl SClient {
    pub fn market(&self) -> impl MarketApi {
        Market_(self.clone())
    }
}

#[async_trait]
impl MarketApi for Market_ {
    async fn currencies(&self) -> Result<Vec<Currency>, Error> {
        self.0.get("/api/v1/currencies", ()).await
    }

    async fn currency(&self, code: &CurrencyCode) -> Result<Currency, Error> {
        self.0
            .get(&format!("/api/v1/currencies/{}", code), ())
            .await
    }

    async fn current_prices_usd(
        &self,
        req: &PricesRequest,
    ) -> Result<HashMap<CurrencyCode, Decimal>, Error> {
        self.0.get("/api/v1/prices", req).await
    }

    async fn klines(&self, req: &KlinesRequest) -> Result<Vec<Kline>, Error> {
        self.0.get("/api/v1/market/candles", req).await
    }

    async fn markets(&self) -> Result<Vec<Market>, Error> {
        self.0.get("/api/v1/symbols", ()).await
    }

    async fn order_book(&self, symbol: &Symbol, depth: OrderBookDepth) -> Result<OrderBook, Error> {
        let url = match depth {
            OrderBookDepth::Partial20 => "/api/v1/market/orderbook/level2_20",
            OrderBookDepth::Partial100 => "/api/v1/market/orderbook/level2_100",
            OrderBookDepth::Full => "/api/v3/market/orderbook/level2",
        };

        self.0.get(&format!("{}?symbol={}", url, symbol), ()).await
    }

    async fn stats_24hr(&self, req: &Stats24hrRequest) -> Result<Vec<Stats24hr>, Error> {
        #[derive(Deserialize)]
        struct Response {
            time: Time,
            ticker: Vec<Stats24hr>,
        }

        if let Stats24hrRequest::Symbol(symbol) = req {
            self.0
                .get(&format!("/api/v1/market/stats?symbol={}", symbol), ())
                .await
                .map(|r| vec![r])
        } else {
            self.0
                .get::<_, Response>("/api/v1/market/allTickers", ())
                .await
                .map(|mut r| {
                    let time = r.time;
                    r.ticker.iter_mut().for_each(|v| {
                        v.time = time;
                    });
                    r.ticker
                })
        }
    }

    async fn ticker(&self, symbol: &Symbol) -> Result<Ticker, Error> {
        self.0
            .get(
                &format!("/api/v1/market/orderbook/level1?symbol={}", symbol),
                (),
            )
            .await
    }

    async fn trades(&self, symbol: &Symbol) -> Result<Vec<Trade>, Error> {
        self.0
            .get(&format!("/api/v1/market/histories?symbol={}", symbol), ())
            .await
    }
}

#[derive(Clone, Debug, Deserialize, Display, Serialize)]
#[display(fmt = "{}", code)]
#[serde(rename_all = "camelCase")]
pub struct Currency {
    #[serde(rename = "currency")]
    pub code: CurrencyCode,
    pub name: String,
    pub full_name: String,
    pub precision: u32,
    pub confirms: usize,
    pub contract_address: String,
    pub withdrawal_min_size: Decimal,
    pub withdrawal_min_fee: Decimal,
    pub is_withdraw_enabled: bool,
    pub is_deposit_enabled: bool,
    pub is_margin_enabled: bool,
    pub is_debit_enabled: bool,
}

impl Eq for Currency {}

impl PartialEq for Currency {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code
    }
}

impl Ord for Currency {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.code.cmp(&other.code)
    }
}

impl PartialOrd for Currency {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl hash::Hash for Currency {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.code.hash(state)
    }
}

#[derive(
    Clone, Debug, Deref, Deserialize, Display, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct CurrencyCode(String);

impl CurrencyCode {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

#[derive(Clone, Debug, Display, Eq, Hash, PartialEq, Serialize)]
#[display(
    fmt = "T: {} O: {} C: {} H: {} L: {} V: {}",
    time,
    open,
    close,
    high,
    low,
    volume
)]
pub struct Kline {
    pub time: Time,
    pub open: Decimal,
    pub close: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub volume: Decimal,
    pub turnover: Decimal,
}

impl Ord for Kline {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.time.cmp(&other.time)
    }
}

impl PartialOrd for Kline {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<'de> de::Deserialize<'de> for Kline {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct TimeStr(Time);

        impl<'de> de::Deserialize<'de> for TimeStr {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                Ok(Self(ts_seconds_str::deserialize(deserializer)?))
            }
        }

        struct KlineVisitor;

        impl<'de> de::Visitor<'de> for KlineVisitor {
            type Value = Kline;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(formatter, "a valid Kline sequence or map")
            }

            fn visit_seq<V>(self, mut seq: V) -> Result<Self::Value, V::Error>
            where
                V: de::SeqAccess<'de>,
            {
                let time: TimeStr = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let time = time.0;
                let open = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let close = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(2, &self))?;
                let high = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(3, &self))?;
                let low = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(4, &self))?;
                let volume = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(5, &self))?;
                let turnover = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(6, &self))?;

                Ok(Kline {
                    time,
                    open,
                    close,
                    high,
                    low,
                    volume,
                    turnover,
                })
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
            where
                V: de::MapAccess<'de>,
            {
                #[derive(Deserialize)]
                #[serde(field_identifier, rename_all = "lowercase")]
                enum Fields {
                    Time,
                    Open,
                    Close,
                    High,
                    Low,
                    Volume,
                    Turnover,
                }

                let mut time = None;
                let mut open = None;
                let mut close = None;
                let mut high = None;
                let mut low = None;
                let mut volume = None;
                let mut turnover = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Fields::Time => time = Some(map.next_value()?),
                        Fields::Open => open = Some(map.next_value()?),
                        Fields::Close => close = Some(map.next_value()?),
                        Fields::High => high = Some(map.next_value()?),
                        Fields::Low => low = Some(map.next_value()?),
                        Fields::Volume => volume = Some(map.next_value()?),
                        Fields::Turnover => turnover = Some(map.next_value()?),
                    }
                }

                let time = time.ok_or_else(|| de::Error::missing_field("time"))?;
                let open = open.ok_or_else(|| de::Error::missing_field("open"))?;
                let close = close.ok_or_else(|| de::Error::missing_field("close"))?;
                let high = high.ok_or_else(|| de::Error::missing_field("high"))?;
                let low = low.ok_or_else(|| de::Error::missing_field("low"))?;
                let volume = volume.ok_or_else(|| de::Error::missing_field("volume"))?;
                let turnover = turnover.ok_or_else(|| de::Error::missing_field("turnover"))?;

                Ok(Kline {
                    time,
                    open,
                    close,
                    high,
                    low,
                    volume,
                    turnover,
                })
            }
        }

        deserializer.deserialize_any(KlineVisitor)
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Display, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(into = "String", try_from = "String")]
pub enum KlineInterval {
    #[display(fmt = "1min")]
    OneMinute,
    #[display(fmt = "3min")]
    ThreeMinute,
    #[display(fmt = "5min")]
    FiveMinute,
    #[display(fmt = "15min")]
    FifteenMinute,
    #[display(fmt = "30min")]
    ThirtyMinute,
    #[display(fmt = "1hour")]
    OneHour,
    #[display(fmt = "2hour")]
    TwoHour,
    #[display(fmt = "4hour")]
    FourHour,
    #[display(fmt = "6hour")]
    SixHour,
    #[display(fmt = "8hour")]
    EightHour,
    #[display(fmt = "12hour")]
    TwelveHour,
    #[display(fmt = "1day")]
    OneDay,
    #[display(fmt = "1week")]
    OneWeek,
}

impl From<KlineInterval> for String {
    fn from(k: KlineInterval) -> Self {
        k.to_string()
    }
}

impl FromStr for KlineInterval {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use KlineInterval::*;

        Ok(match s {
            "1min" => OneMinute,
            "3min" => ThreeMinute,
            "5min" => FiveMinute,
            "15min" => FifteenMinute,
            "30min" => ThirtyMinute,
            "1hour" => OneHour,
            "2hour" => TwoHour,
            "4hour" => FourHour,
            "6hour" => SixHour,
            "8hour" => EightHour,
            "12hour" => TwelveHour,
            "1day" => OneDay,
            "1week" => OneWeek,
            _ => ParseSnafu {
                reason: format!("The kline interval '{}' is not recognized", s),
            }
            .fail()?,
        })
    }
}

impl TryFrom<String> for KlineInterval {
    type Error = <KlineInterval as FromStr>::Err;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KlinesRequest {
    symbol: Symbol,
    #[serde(with = "ts_seconds")]
    start_at: Time,
    #[serde(with = "ts_seconds")]
    end_at: Time,
    #[serde(rename = "type")]
    interval: KlineInterval,
}

impl KlinesRequest {
    pub fn new(symbol: Symbol, start_at: Time, end_at: Time, interval: KlineInterval) -> Self {
        Self {
            symbol,
            start_at,
            end_at,
            interval,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Display, Serialize)]
#[display(fmt = "{} ({})", symbol, market)]
#[serde(rename_all = "camelCase")]
pub struct Market {
    pub symbol: Symbol,
    pub name: String,
    pub base_currency: CurrencyCode,
    pub quote_currency: CurrencyCode,
    pub market: String,
    pub base_min_size: Decimal,
    pub quote_min_size: Decimal,
    pub base_max_size: Decimal,
    pub quote_max_size: Decimal,
    pub base_increment: Decimal,
    pub quote_increment: Decimal,
    pub price_increment: Decimal,
    pub price_limit_rate: Decimal,
    pub fee_currency: CurrencyCode,
    pub enable_trading: bool,
    pub is_margin_enabled: bool,
}

impl Eq for Market {}

impl PartialEq for Market {
    fn eq(&self, other: &Self) -> bool {
        self.symbol == other.symbol
    }
}

impl Ord for Market {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.symbol.cmp(&other.symbol)
    }
}

impl PartialOrd for Market {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl hash::Hash for Market {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.symbol.hash(state);
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderBook {
    #[serde(default)]
    pub sequence: Decimal,
    #[serde(alias = "timestamp")]
    pub time: Time,
    #[serde(deserialize_with = "deserialize_bids_asks")]
    pub asks: BTreeMap<Decimal, Decimal>,
    #[serde(deserialize_with = "deserialize_bids_asks")]
    pub bids: BTreeMap<Decimal, Decimal>,
}

impl Eq for OrderBook {}

impl PartialEq for OrderBook {
    fn eq(&self, other: &Self) -> bool {
        self.sequence == other.sequence && self.time == other.time
    }
}

impl Ord for OrderBook {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        match self.sequence.cmp(&other.sequence) {
            cmp::Ordering::Equal => self.time.cmp(&other.time),
            ordering => ordering,
        }
    }
}

impl PartialOrd for OrderBook {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl hash::Hash for OrderBook {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.sequence.hash(state);
        self.time.hash(state);
    }
}

fn deserialize_bids_asks<'de, D>(deserializer: D) -> Result<BTreeMap<Decimal, Decimal>, D::Error>
where
    D: de::Deserializer<'de>,
{
    let v: Vec<(Decimal, Decimal)> = Deserialize::<'de>::deserialize(deserializer)?;
    Ok(v.into_iter().collect())
}

#[derive(Clone, Copy, Debug)]
pub enum OrderBookDepth {
    Partial20,
    Partial100,
    Full,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PricesRequest {
    currencies: Option<String>,
}

impl PricesRequest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn currencies<T>(self, currencies: impl IntoIterator<Item = T>) -> Self
    where
        T: Borrow<CurrencyCode> + ToString,
    {
        let currencies = Some(iter_to_csv(currencies));
        Self { currencies, ..self }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats24hr {
    #[serde(default)]
    pub time: Time,
    pub symbol: Symbol,
    pub buy: Decimal,
    pub sell: Decimal,
    pub change_rate: Decimal,
    pub change_price: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    #[serde(rename = "vol")]
    pub vol_base: Decimal,
    #[serde(rename = "volValue")]
    pub vol_quote: Decimal,
    pub last: Decimal,
    pub average_price: Decimal,
    pub taker_fee_rate: Decimal,
    pub maker_fee_rate: Decimal,
    pub taker_coefficient: Decimal,
    pub maker_coefficient: Decimal,
}

impl Eq for Stats24hr {}

impl PartialEq for Stats24hr {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time && self.symbol == other.symbol
    }
}

impl Ord for Stats24hr {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        match self.time.cmp(&other.time) {
            cmp::Ordering::Equal => self.symbol.cmp(&other.symbol),
            ordering => ordering,
        }
    }
}

impl PartialOrd for Stats24hr {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl hash::Hash for Stats24hr {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.time.hash(state);
        self.symbol.hash(state);
    }
}

#[derive(Clone, Debug)]
pub enum Stats24hrRequest {
    All,
    Symbol(Symbol),
}

#[derive(Clone, Debug, Deserialize, Display, Eq, Hash, PartialEq, Serialize)]
#[display(fmt = "{}-{}", base, quote)]
#[serde(into = "String", try_from = "String")]
pub struct Symbol {
    base: CurrencyCode,
    quote: CurrencyCode,
}

impl Symbol {
    pub fn new(base: CurrencyCode, quote: CurrencyCode) -> Self {
        Self { base, quote }
    }
}

impl Ord for Symbol {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        match self.quote.cmp(&other.quote) {
            cmp::Ordering::Equal => self.base.cmp(&other.base),
            ordering => ordering,
        }
    }
}

impl PartialOrd for Symbol {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl From<Symbol> for String {
    fn from(s: Symbol) -> Self {
        s.to_string()
    }
}

impl FromStr for Symbol {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(2, '-');
        parts
            .next()
            .zip(parts.next())
            .map(|(base, quote)| Symbol::new(CurrencyCode::new(base), CurrencyCode::new(quote)))
            .ok_or_else(|| {
                ParseSnafu {
                    reason: format!("The symbol '{}' could not be parsed.", s),
                }
                .build()
            })
    }
}

impl TryFrom<String> for Symbol {
    type Error = <Symbol as FromStr>::Err;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Display, Serialize)]
#[display(fmt = "{}: {} @ {}", time, size, price)]
#[serde(rename_all = "camelCase")]
pub struct Ticker {
    #[serde(default = "Time::now")]
    pub time: Time,
    pub sequence: Decimal,
    pub size: Decimal,
    pub price: Decimal,
    pub best_bid_size: Decimal,
    pub best_bid: Decimal,
    pub best_ask_size: Decimal,
    pub best_ask: Decimal,
}

impl Eq for Ticker {}

impl PartialEq for Ticker {
    fn eq(&self, other: &Self) -> bool {
        self.sequence == other.sequence && self.time == other.time
    }
}

impl Ord for Ticker {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        match self.sequence.cmp(&other.sequence) {
            cmp::Ordering::Equal => self.time.cmp(&other.time),
            ordering => ordering,
        }
    }
}

impl PartialOrd for Ticker {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl hash::Hash for Ticker {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.sequence.hash(state);
        self.time.hash(state);
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
