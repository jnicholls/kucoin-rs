use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedRequest {
    pub current_page: usize,
    pub page_size: usize,
}

impl Default for PagedRequest {
    fn default() -> Self {
        Self {
            current_page: 1,
            page_size: 50,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedResponse {
    pub current_page: usize,
    pub page_size: usize,
    pub total_num: usize,
    pub total_page: usize,
}
