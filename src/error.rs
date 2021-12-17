use std::fmt;

use derive_more::{Display, From};
use serde::{de, ser, Deserialize};
use snafu::Snafu;

#[derive(Clone, Copy, Debug, Display, Eq, From, Hash, PartialEq)]
#[display(fmt = "{}", _0)]
pub struct SystemCode(pub(crate) u32);

impl<'de> de::Deserialize<'de> for SystemCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct CodeVisitor;

        impl<'de> de::Visitor<'de> for CodeVisitor {
            type Value = SystemCode;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a raw or string unsigned 32-bit integer")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let n: u32 = v.parse().map_err(|_| {
                    E::invalid_value(de::Unexpected::Str(v), &"an unsigned 32-bit integer string")
                })?;
                self.visit_u64(n as u64)
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if v > u64::from(u32::MAX) {
                    Err(E::invalid_value(
                        de::Unexpected::Unsigned(v),
                        &"an unsigned 32-bit integer",
                    ))
                } else {
                    Ok((v as u32).into())
                }
            }
        }

        deserializer.deserialize_any(CodeVisitor)
    }
}

impl ser::Serialize for SystemCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u32(self.0)
    }
}

#[derive(Clone, Debug, Deserialize, Snafu)]
#[snafu(display("({}) {}", code, msg))]
pub struct SystemError {
    pub(crate) code: SystemCode,
    pub(crate) msg: String,
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum Error {
    #[snafu(display("Bad request: {}", source))]
    BadRequest { source: SystemError },

    #[snafu(display("The request is forbidden: {}", source))]
    Forbidden { source: SystemError },

    #[snafu(display("HTTP request error: {}", source))]
    HttpRequest { source: reqwest::Error },

    #[snafu(display("Incorrect Content-Type: {}", source))]
    IncorrectContentType { source: SystemError },

    #[snafu(display("An internal server error occurred: {}", source))]
    InternalServer { source: SystemError },

    #[snafu(display("JSON encoding error: {}", source))]
    JsonEncoding { source: serde_json::Error },

    #[snafu(display("Method not found: {}", source))]
    MethodNotAllowed { source: SystemError },

    #[snafu(display("Not found: {}", source))]
    NotFound { source: SystemError },

    #[snafu(display("{}", reason))]
    Parse { reason: String },

    #[snafu(display(
        "Unable to decode the JSON response:\n  JSON: {}\n  Error: {}",
        resp_json,
        source
    ))]
    ResponseDecoding {
        resp_json: String,
        source: serde_json::Error,
    },

    #[snafu(display("Service is unavailable: {}", source))]
    ServiceUnavailable { source: SystemError },

    #[snafu(display("Too many requests: {}", source))]
    TooManyRequests { source: SystemError },

    #[snafu(display("Unauthorized API key: {}", source))]
    Unauthorized { source: SystemError },

    #[snafu(display("An unexpected status was returned: ({}) {}", status, source))]
    UnexpectedStatus { status: u16, source: SystemError },

    #[snafu(display("URL encoding error: {}", source))]
    UrlEncoding {
        source: serde_urlencoded::ser::Error,
    },

    #[snafu(display("Websocket error: {}", source))]
    Websocket {
        source: async_tungstenite::tungstenite::Error,
    },

    #[snafu(display("The websocket connection is closed"))]
    WebsocketClosed,

    #[snafu(display("Unable to decode the websocket message: {}", source))]
    WebsocketMessageDecoding {
        source: serde_value::DeserializerError,
    },

    #[snafu(display("No websocket server is available at this time."))]
    WebsocketUnavailable,
}
