#![deny(unsafe_op_in_unsafe_fn, missing_docs)]

//! Hyena is a simple, fast, and safe async task pool.
//!
//! # Examples
//! ```rust
//! # use hyena::TaskPool;
//! // create a new task pool
//! let task_pool = TaskPool::new().unwrap();
//!
//! // spawn a task
//! let task = task_pool.spawn(async {
//!     // do some async work
//!     2 + 2
//! });
//!
//! // wait for the task to complete
//! let result = task.block_on();
//! assert_eq!(result, 4);
//! ```

mod task;
mod task_pool;

pub use task::*;
pub use task_pool::*;

use std::future::Future;

/// Spawns a [`Task`] on the global [`TaskPool`].
#[inline]
pub fn spawn<T>(future: impl Future<Output = T> + Send + 'static) -> Task<T>
where
    T: Send + 'static,
{
    TaskPool::global().spawn(future)
}

/// Spawns a [`Task`] on the thread local executor.
#[inline]
pub fn spawn_local<T>(future: impl Future<Output = T> + 'static) -> Task<T>
where
    T: 'static,
{
    TaskPool::global().spawn_local(future)
}

/// Creates a [`Scope`] on the global [`TaskPool`].
#[inline]
pub fn scope<'env, F, T>(f: F) -> Vec<T>
where
    F: for<'scope> FnOnce(&'scope Scope<'scope, 'env, T>),
    T: Send + 'static,
{
    TaskPool::global().scope(f)
}
