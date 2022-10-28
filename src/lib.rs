#![deny(unsafe_op_in_unsafe_fn)]

//! # Hyena
//! Hyena is a simple, fast, and safe async task pool.
//!
//! # Examples
//! ```rust
//! #use hyena::TaskPool;
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
//! let result = task_pool.block_on(task);
//! assert_eq!(result, 4);
//! ```

mod task;
mod task_pool;

pub use task::*;
pub use task_pool::*;
