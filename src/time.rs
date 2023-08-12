use std::fmt;

use derive_more::{Constructor, Deref, DerefMut, From, Into};
use serde::{de, ser, Deserialize, Serialize};
use serde_with::{
    formats::Format, serde_as, DeserializeAs, SerializeAs, TimestampMilliSeconds,
    TimestampNanoSeconds, TimestampSeconds,
};
use time::OffsetDateTime;

#[serde_as]
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
    Into,
    PartialEq,
    PartialOrd,
    Ord,
    Serialize,
)]
#[serde(transparent)]
pub struct Time(#[serde_as(as = "TimestampMilliSeconds")] OffsetDateTime);

impl Time {
    pub fn now() -> Self {
        OffsetDateTime::now_utc().into()
    }
}

impl Default for Time {
    fn default() -> Self {
        OffsetDateTime::UNIX_EPOCH.into()
    }
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ms = self.unix_timestamp() * 1000 + self.millisecond() as i64;
        write!(f, "{ms}")
    }
}

impl<'de, F> DeserializeAs<'de, Time> for TimestampNanoSeconds<F>
where
    F: Format,
    Self: DeserializeAs<'de, OffsetDateTime>,
{
    fn deserialize_as<D>(deserializer: D) -> Result<Time, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        Ok(Time(Self::deserialize_as(deserializer)?))
    }
}

impl<F> SerializeAs<Time> for TimestampNanoSeconds<F>
where
    F: Format,
    Self: SerializeAs<OffsetDateTime>,
{
    fn serialize_as<S>(source: &Time, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        Self::serialize_as(&source.0, serializer)
    }
}

impl<'de, F> DeserializeAs<'de, Time> for TimestampSeconds<F>
where
    F: Format,
    Self: DeserializeAs<'de, OffsetDateTime>,
{
    fn deserialize_as<D>(deserializer: D) -> Result<Time, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        Ok(Time(Self::deserialize_as(deserializer)?))
    }
}

impl<F> SerializeAs<Time> for TimestampSeconds<F>
where
    F: Format,
    Self: SerializeAs<OffsetDateTime>,
{
    fn serialize_as<S>(source: &Time, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        Self::serialize_as(&source.0, serializer)
    }
}
