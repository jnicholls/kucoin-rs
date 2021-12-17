use std::fmt;

use chrono::{serde as chrono_serde, DateTime, TimeZone, Utc};
use derive_more::{Constructor, Deref, DerefMut, From};
use serde::{de, ser, Deserialize, Serialize};

#[derive(
    Clone,
    Constructor,
    Copy,
    Debug,
    Deref,
    DerefMut,
    Deserialize,
    Eq,
    From,
    Hash,
    PartialEq,
    PartialOrd,
    Ord,
    Serialize,
)]
#[serde(transparent)]
pub struct Time(#[serde(with = "chrono_serde::ts_milliseconds")] DateTime<Utc>);

impl Time {
    pub fn now() -> Self {
        Utc::now().into()
    }
}

impl Default for Time {
    fn default() -> Self {
        Self(Utc.timestamp(0, 0))
    }
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.timestamp_millis())
    }
}

pub mod ts_nanoseconds {
    use super::*;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Time, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let dt = chrono_serde::ts_nanoseconds::deserialize(deserializer)?;
        Ok(Time(dt))
    }

    pub fn serialize<S>(time: &Time, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        chrono_serde::ts_nanoseconds::serialize(&time.0, serializer)
    }
}

pub mod ts_nanoseconds_str {
    use super::*;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Time, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let ns: i64 = s.parse().map_err(|_| {
            de::Error::invalid_value(de::Unexpected::Str(&s), &"an epoch time in nanoseconds")
        })?;
        let dt = Utc.timestamp_nanos(ns);
        Ok(Time(dt))
    }

    pub fn serialize<S>(time: &Time, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        let ts = time.timestamp_nanos().to_string();
        ts.serialize(serializer)
    }
}

pub mod ts_seconds {
    use super::*;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Time, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let dt = chrono_serde::ts_seconds::deserialize(deserializer)?;
        Ok(Time(dt))
    }

    pub fn serialize<S>(time: &Time, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        chrono_serde::ts_seconds::serialize(&time.0, serializer)
    }
}

pub mod ts_seconds_str {
    use super::*;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Time, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        let time = Utc
            .datetime_from_str(&s, "%s")
            .map_err(|_| {
                de::Error::invalid_value(
                    de::Unexpected::Str(&s),
                    &"an epoch time expressed in seconds",
                )
            })?
            .into();

        Ok(time)
    }

    pub fn serialize<S>(time: &Time, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        let ts = time.timestamp().to_string();
        ts.serialize(serializer)
    }
}
