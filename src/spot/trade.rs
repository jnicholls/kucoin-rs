use derive_more::{Deref, Display};
use serde::{Deserialize, Serialize};

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
