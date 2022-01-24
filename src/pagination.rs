use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::stream::{self, BoxStream, Stream, StreamExt};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{
    client::{Api, Client},
    error::Error,
};

impl<A> Client<A>
where
    A: Api,
{
    pub(crate) fn paged_get<'a, I, O>(&'a self, path: &'a str, req: I) -> PagedStream<'a, O>
    where
        I: Serialize + Send + Sync + 'a,
        O: DeserializeOwned,
    {
        PagedStream::new(stream::try_unfold(
            (PagedRequest::new(req), 1, usize::MAX),
            move |(paged_req, current_page, total_page)| async move {
                if current_page > total_page {
                    Ok(None)
                } else {
                    let paged_req = paged_req.current_page(current_page);
                    let resp = self.get::<_, PagedResponse<O>>(path, &paged_req).await?;
                    Ok(Some((
                        resp.items,
                        (paged_req, current_page + 1, resp.total_page),
                    )))
                }
            },
        ))
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedRequest<T> {
    current_page: usize,
    page_size: usize,
    #[serde(flatten)]
    req: T,
}

impl<T> PagedRequest<T> {
    pub fn new(req: T) -> Self {
        Self {
            current_page: 1,
            page_size: 500,
            req,
        }
    }

    pub fn current_page(self, current_page: usize) -> Self {
        Self {
            current_page,
            ..self
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedResponse<T> {
    pub current_page: usize,
    pub page_size: usize,
    pub total_num: usize,
    pub total_page: usize,
    pub items: Vec<T>,
}

pub struct PagedStream<'a, T> {
    stream: BoxStream<'a, Result<Vec<T>, Error>>,
}

impl<'a, T> PagedStream<'a, T> {
    pub fn new<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<Vec<T>, Error>> + Send + 'a,
    {
        let stream = stream.boxed();
        Self { stream }
    }
}

impl<'a, T> Stream for PagedStream<'a, T> {
    type Item = Result<Vec<T>, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.stream).poll_next(cx)
    }
}
