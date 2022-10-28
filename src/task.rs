use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use futures_lite::future;

#[derive(Debug)]
#[must_use = "Tasks are cancelled when they are dropped"]
pub struct Task<T> {
    inner: async_executor::Task<T>,
}

impl<T> Task<T> {
    #[inline]
    pub fn new(inner: async_executor::Task<T>) -> Self {
        Self { inner }
    }

    #[inline]
    pub fn detach(self) {
        self.inner.detach();
    }

    #[inline]
    pub async fn cancel(self) {
        self.inner.cancel().await;
    }

    #[inline]
    pub fn cancel_blocking(self) {
        future::block_on(self.cancel());
    }

    #[inline]
    pub fn block_on(self) -> T {
        future::block_on(self.inner)
    }
}

impl<T> Future for Task<T> {
    type Output = T;

    #[inline]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.inner).poll(cx)
    }
}
