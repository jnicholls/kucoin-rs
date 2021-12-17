use std::borrow::Borrow;
use std::cmp;
use std::collections::HashMap;
use std::hash;

use async_trait::async_trait;
use derive_more::{Deref, Display};
use futures_util::{
    future::{self, Either},
    stream::FuturesUnordered,
    TryFutureExt, TryStreamExt,
};
use num_derive::FromPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    client::SClient,
    error::Error,
    spot::{Currency, CurrencyCode, OrderId, Symbol},
    utils::iter_to_csv,
};

#[async_trait]
pub trait UserApi {
    async fn accounts(&self, req: &AccountsRequest) -> Result<Vec<Account>, Error>;

    async fn base_fee(&self) -> Result<Fee, Error>;

    async fn create_account(
        &self,
        account_type: AccountType,
        currency: CurrencyCode,
    ) -> Result<AccountId, Error>;

    async fn refresh_account(&self, account: &mut Account) -> Result<(), Error>;

    async fn trade_fees<'a, S>(
        &'a self,
        symbols: impl IntoIterator<Item = S> + Send + 'a,
    ) -> Result<HashMap<Symbol, Fee>, Error>
    where
        S: Borrow<Symbol> + ToString;

    async fn transfer(&self, req: &TransferRequest) -> Result<OrderId, Error>;
}

struct User(SClient);

impl SClient {
    pub fn user(&self) -> impl UserApi {
        User(self.clone())
    }
}

#[async_trait]
impl UserApi for User {
    async fn accounts(&self, req: &AccountsRequest) -> Result<Vec<Account>, Error> {
        self.0
            .get::<_, Vec<_>>("/api/v1/accounts", req)
            .and_then(|accounts| {
                if req.refresh_all {
                    Either::Left(
                        accounts
                            .into_iter()
                            .map(|mut account| async move {
                                self.refresh_account(&mut account).await?;
                                Ok(account)
                            })
                            .collect::<FuturesUnordered<_>>()
                            .try_collect(),
                    )
                } else {
                    Either::Right(future::ok(accounts))
                }
            })
            .await
    }

    async fn base_fee(&self) -> Result<Fee, Error> {
        self.0.get("/api/v1/base-fee", ()).await
    }

    async fn create_account(
        &self,
        account_type: AccountType,
        currency: CurrencyCode,
    ) -> Result<AccountId, Error> {
        #[derive(Deserialize)]
        struct Response {
            id: AccountId,
        }

        self.0
            .post::<_, Response>(
                "/api/v1/accounts",
                json!({
                    "type": account_type,
                    "currency": currency,
                }),
            )
            .await
            .map(|r| r.id)
    }

    async fn refresh_account(&self, account: &mut Account) -> Result<(), Error> {
        #[derive(Deserialize)]
        struct Response {
            balance: Decimal,
            available: Decimal,
            holds: Decimal,
            transferable: Decimal,
        }

        self.0
            .get::<_, Response>(
                "/api/v1/accounts/transferable",
                json!({
                    "currency": account.currency,
                    "type": account.account_type.to_string(),
                }),
            )
            .await
            .map(|r| {
                account.balance = r.balance;
                account.available = r.available;
                account.holds = r.holds;
                account.transferable = r.transferable;
            })
    }

    async fn trade_fees<'a, S>(
        &'a self,
        symbols: impl IntoIterator<Item = S> + Send + 'a,
    ) -> Result<HashMap<Symbol, Fee>, Error>
    where
        S: Borrow<Symbol> + ToString,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            symbol: Symbol,
            taker_fee_rate: Decimal,
            maker_fee_rate: Decimal,
        }

        self.0
            .get::<_, Vec<Response>>(
                &format!("/api/v1/trade-fees?symbols={}", iter_to_csv(symbols)),
                (),
            )
            .await
            .map(|symbols| {
                symbols
                    .into_iter()
                    .map(|s| {
                        (
                            s.symbol,
                            Fee {
                                taker_fee_rate: s.taker_fee_rate,
                                maker_fee_rate: s.maker_fee_rate,
                            },
                        )
                    })
                    .collect()
            })
    }

    async fn transfer(&self, req: &TransferRequest) -> Result<OrderId, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            order_id: OrderId,
        }

        self.0
            .post::<_, Response>("/api/v2/accounts/inner-transfer", req)
            .await
            .map(|r| r.order_id)
    }
}

#[derive(Clone, Debug, Deserialize, Display, Serialize)]
#[display(fmt = "{} ({} {})", id, currency, account_type)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: AccountId,
    pub currency: CurrencyCode,
    #[serde(rename = "type")]
    pub account_type: AccountType,
    pub balance: Decimal,
    pub available: Decimal,
    pub holds: Decimal,
    #[serde(default)]
    pub transferable: Decimal,
}

impl Eq for Account {}

impl PartialEq for Account {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Ord for Account {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        match self.account_type.cmp(&other.account_type) {
            cmp::Ordering::Equal => self.currency.cmp(&other.currency),
            ordering => ordering,
        }
    }
}

impl PartialOrd for Account {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl hash::Hash for Account {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

#[derive(Clone, Debug, Deref, Deserialize, Display, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct AccountId(String);

impl AccountId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Display,
    Eq,
    FromPrimitive,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
)]
#[serde(rename_all = "camelCase")]
pub enum AccountType {
    #[display(fmt = "MAIN")]
    Main,
    #[display(fmt = "TRADE")]
    Trade,
    #[display(fmt = "MARGIN")]
    Margin,
    #[display(fmt = "POOL")]
    Pool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct AccountsRequest {
    #[serde(skip)]
    refresh_all: bool,
    currency: Option<CurrencyCode>,
    #[serde(rename = "type")]
    account_type: Option<AccountType>,
}

impl AccountsRequest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn refresh_all(self) -> Self {
        Self {
            refresh_all: true,
            ..self
        }
    }

    pub fn currency(self, currency: CurrencyCode) -> Self {
        Self {
            currency: Some(currency),
            ..self
        }
    }

    pub fn account_type(self, account_type: AccountType) -> Self {
        Self {
            account_type: Some(account_type),
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fee {
    pub taker_fee_rate: Decimal,
    pub maker_fee_rate: Decimal,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferRequest {
    #[serde(rename = "clientOid")]
    client_order_id: String,
    currency: CurrencyCode,
    from: AccountType,
    to: AccountType,
    amount: Decimal,
}

impl TransferRequest {
    pub fn new(
        client_order_id: impl Into<String>,
        currency: &Currency,
        from: AccountType,
        to: AccountType,
        amount: Decimal,
    ) -> Result<Self, Error> {
        let amount = amount.round_dp(currency.precision);

        Ok(Self {
            client_order_id: client_order_id.into(),
            currency: currency.code.clone(),
            from,
            to,
            amount,
        })
    }
}
