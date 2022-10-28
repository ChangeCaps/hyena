use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use futures_lite::future;

/// A task that can be spawned onto a [`TaskPool`](crate::TaskPool).
///
/// **Note** Tasks are cancelled when dropped.
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

    /// Returns true if the task has completed.
    #[inline]
    pub fn is_finished(&self) -> bool {
        self.inner.is_finished()
    }

    /// Detaches the task, causing it to run in the background.
    #[inline]
    pub fn detach(self) {
        self.inner.detach();
    }

    /// Cancels the task and waits for it to cancel. As opposed to dropping it.
    #[inline]
    pub async fn cancel(self) {
        self.inner.cancel().await;
    }

    /// Cancels the task and waits for it to cancel.
    ///
    /// This is equivalent to blocking on [`Task::cancel`](Task::cancel).
    #[inline]
    pub fn cancel_blocking(self) {
        future::block_on(self.cancel());
    }

    /// Runs the task to completion.
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
