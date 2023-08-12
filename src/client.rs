use std::fmt;
use std::marker::PhantomData;

use base64::prelude::{Engine, BASE64_STANDARD};
use hmac::{Hmac, Mac};
use serde::{de::DeserializeOwned, ser::Serialize, Deserialize};
use sha2::Sha256;
use snafu::ResultExt;

use crate::{error::*, time::Time};

const FUTURES_API: &str = "https://api-futures.kucoin.com";
const SPOT_API: &str = "https://api.kucoin.com";

pub type FClient = Client<FApi>;
pub type SClient = Client<SApi>;

macro_rules! http_verb {
    ($method:ident) => {
        #[allow(dead_code)]
        pub(crate) async fn $method<I, O>(&self, path: &str, data: I) -> Result<O, Error>
        where
            I: Serialize,
            O: DeserializeOwned,
        {
            let host = A::host();
            let method = stringify!($method).to_ascii_uppercase();
            let payload = serde_json::to_string(&data).context(JsonEncodingSnafu)?;
            let endpoint = path;

            let url = format!("{host}{endpoint}");
            let req = self.http.$method(&url);

            let req = if let Some(creds) = &self.creds {
                let timestamp = Time::now().to_string();
                let prehash = format!("{timestamp}{method}{endpoint}{payload}");
                req.header("KC-API-KEY", creds.key())
                    .header("KC-API-SIGN", creds.sign(prehash))
                    .header("KC-API-TIMESTAMP", timestamp)
                    .header("KC-API-PASSPHRASE", creds.sign(creds.passphrase()))
                    .header("KC-API-KEY-VERSION", "2")
            } else {
                req
            };

            let req = req.header("Content-Type", "application/json").body(payload);
            self.send_request(req).await
        }
    };

    ($method:ident?) => {
        #[allow(dead_code)]
        pub(crate) async fn $method<I, O>(&self, path: &str, data: I) -> Result<O, Error>
        where
            I: Serialize,
            O: DeserializeOwned,
        {
            let host = A::host();
            let method = stringify!($method).to_ascii_uppercase();
            let query = serde_urlencoded::to_string(data).context(UrlEncodingSnafu)?;
            let endpoint = if query.is_empty() {
                path.to_string()
            } else {
                format!("{path}?{query}")
            };

            let url = format!("{host}{endpoint}");
            let req = self.http.$method(&url);

            let req = if let Some(creds) = &self.creds {
                let timestamp = Time::now().to_string();
                let prehash = format!("{timestamp}{method}{endpoint}");
                req.header("KC-API-KEY", creds.key())
                    .header("KC-API-SIGN", creds.sign(prehash))
                    .header("KC-API-TIMESTAMP", timestamp)
                    .header("KC-API-PASSPHRASE", creds.sign(creds.passphrase()))
                    .header("KC-API-KEY-VERSION", "2")
            } else {
                req
            };

            self.send_request(req).await
        }
    };
}

pub trait Api: Send + Sync + 'static {
    fn host() -> &'static str;
}

pub struct FApi;

impl Api for FApi {
    fn host() -> &'static str {
        FUTURES_API
    }
}

pub struct SApi;

impl Api for SApi {
    fn host() -> &'static str {
        SPOT_API
    }
}

pub struct Client<A> {
    creds: Option<Credentials>,
    http: reqwest::Client,
    _marker: PhantomData<A>,
}

impl<A> Client<A> {
    pub fn new() -> Self {
        Self {
            creds: None,
            http: reqwest::Client::new(),
            _marker: PhantomData,
        }
    }

    pub fn with_credentials(creds: Credentials) -> Self {
        let creds = Some(creds);

        Self {
            creds,
            http: reqwest::Client::new(),
            _marker: PhantomData,
        }
    }

    pub(crate) fn has_credentials(&self) -> bool {
        self.creds.is_some()
    }

    async fn send_request<O>(&self, req: reqwest::RequestBuilder) -> Result<O, Error>
    where
        O: DeserializeOwned,
    {
        #[derive(Deserialize)]
        struct Response<T> {
            code: SystemCode,
            data: Option<T>,
            msg: Option<String>,
        }

        let resp = req.send().await.context(HttpRequestSnafu)?;
        let resp_status = resp.status();
        let resp_json = resp.text().await.context(HttpRequestSnafu)?;

        match resp_status.as_u16() {
            200 => {
                let resp: Response<O> = serde_json::from_str(&resp_json)
                    .context(ResponseDecodingSnafu { resp_json })?;

                match (resp.code, resp.data) {
                    (SystemCode(200000), Some(data)) => Ok(data),
                    _ => {
                        let se = SystemError {
                            code: resp.code,
                            msg: resp.msg.unwrap_or("An unknown error occurred.".to_string()),
                        };
                        Err(se).context(BadRequestSnafu)
                    }
                }
            }
            status => {
                let se: Result<_, SystemError> = Err(serde_json::from_str(&resp_json)
                    .context(ResponseDecodingSnafu { resp_json })?);
                match status {
                    400 => se.context(BadRequestSnafu),
                    401 => se.context(UnauthorizedSnafu),
                    403 => se.context(ForbiddenSnafu),
                    404 => se.context(NotFoundSnafu),
                    405 => se.context(MethodNotAllowedSnafu),
                    415 => se.context(IncorrectContentTypeSnafu),
                    429 => se.context(TooManyRequestsSnafu),
                    500 => se.context(InternalServerSnafu),
                    503 => se.context(ServiceUnavailableSnafu),
                    _ => se.context(UnexpectedStatusSnafu { status }),
                }
            }
        }
    }
}

impl<A> Client<A>
where
    A: Api,
{
    http_verb!(delete?);
    http_verb!(get?);
    http_verb!(patch);
    http_verb!(post);
    http_verb!(put);
}

impl<A> Clone for Client<A> {
    fn clone(&self) -> Self {
        Self {
            creds: self.creds.clone(),
            http: self.http.clone(),
            _marker: PhantomData,
        }
    }
}

impl<A> fmt::Debug for Client<A> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Client")
            .field("creds", &self.creds)
            .field("http", &self.http)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct Credentials {
    key: String,
    passphrase: String,
    secret: String,
}

impl Credentials {
    pub fn new(key: impl ToString, passphrase: impl ToString, secret: impl ToString) -> Self {
        let key = key.to_string();
        let passphrase = passphrase.to_string();
        let secret = secret.to_string();

        Self {
            key,
            passphrase,
            secret,
        }
    }

    fn key(&self) -> &str {
        &self.key
    }

    fn passphrase(&self) -> &str {
        &self.passphrase
    }

    fn sign(&self, data: impl AsRef<[u8]>) -> String {
        let mut hmac = Hmac::<Sha256>::new_from_slice(self.secret.as_bytes())
            .expect("secret should be of the correct byte length");
        hmac.update(data.as_ref());
        BASE64_STANDARD.encode(hmac.finalize().into_bytes())
    }
}
