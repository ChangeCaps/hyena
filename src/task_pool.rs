use std::{future::Future, io, sync::Arc, thread::JoinHandle};

use async_executor::{Executor, LocalExecutor};
use futures_lite::future;

use crate::Task;

#[must_use = "TaskPoolBuilder does nothing unless you call `build`"]
#[derive(Clone, Debug, Default)]
pub struct TaskPoolBuilder {
    num_threads: Option<usize>,
    stack_size: Option<usize>,
    thread_name: Option<String>,
}

impl TaskPoolBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the number of threads to use.
    ///
    /// If not set, the number of threads will be equal to the number of logical cores.
    pub fn num_threads(mut self, num_threads: usize) -> Self {
        self.num_threads = Some(num_threads);
        self
    }

    /// Sets the stack size of each thread.
    pub fn stack_size(mut self, stack_size: usize) -> Self {
        self.stack_size = Some(stack_size);
        self
    }

    /// Sets the name of each thread.
    ///
    /// The name will be suffixed with a number.
    pub fn thread_name(mut self, thread_name: impl Into<String>) -> Self {
        self.thread_name = Some(thread_name.into());
        self
    }

    #[must_use]
    pub fn build(self) -> io::Result<TaskPool> {
        TaskPool::new_internal(
            self.num_threads,
            self.stack_size,
            self.thread_name.as_deref(),
        )
    }
}

#[derive(Debug)]
struct TaskPoolInner {
    threads: Vec<JoinHandle<()>>,
    shutdown: async_channel::Sender<()>,
}

impl Drop for TaskPoolInner {
    fn drop(&mut self) {
        self.shutdown.close();

        let panicking = std::thread::panicking();
        for thread in self.threads.drain(..) {
            let result = thread.join();

            if !panicking {
                result.expect("Task thread panicked.");
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct TaskPool {
    executor: Arc<Executor<'static>>,
    inner: Arc<TaskPoolInner>,
}

impl TaskPool {
    thread_local! {
        static LOCAL_EXECUTOR: LocalExecutor<'static> = LocalExecutor::new();
    }

    /// Creates a new [`TaskPool`] using default settings.
    pub fn new() -> io::Result<Self> {
        TaskPoolBuilder::new().build()
    }

    /// Creates a new [`TaskPoolBuilder`].
    ///
    /// # Examples
    /// ```rust
    /// #use hyena::TaskPool;
    /// let task_pool = TaskPool::builder()
    ///     .num_threads(4)
    ///     .stack_size(1024 * 1024)
    ///     .thread_name("my-task-pool")
    ///     .build()
    ///     .expect("Failed to create task pool");
    /// ```
    pub fn builder() -> TaskPoolBuilder {
        TaskPoolBuilder::new()
    }

    fn new_internal(
        num_threads: Option<usize>,
        stack_size: Option<usize>,
        thread_name: Option<&str>,
    ) -> io::Result<Self> {
        let (shutdown, shutdown_rx) = async_channel::bounded(1);

        let executor = Arc::new(Executor::new());

        let num_threads = num_threads.unwrap_or_else(num_cpus::get);

        let threads: Vec<_> = (0..num_threads)
            .map(|i| {
                let executor = executor.clone();
                let shutdown_rx = shutdown_rx.clone();
                let thread_name = if let Some(thread_name) = thread_name {
                    format!("{}({})", thread_name, i)
                } else {
                    format!("TaskPool({})", i)
                };

                let mut thread_builder = std::thread::Builder::new().name(thread_name);

                if let Some(stack_size) = stack_size {
                    thread_builder = thread_builder.stack_size(stack_size);
                }

                thread_builder.spawn(move || {
                    let fut = executor.run(shutdown_rx.recv());
                    future::block_on(fut).unwrap_err();
                })
            })
            .try_fold(Vec::new(), |mut threads, thread| {
                threads.push(thread?);
                io::Result::Ok(threads)
            })?;

        Ok(Self {
            executor,
            inner: Arc::new(TaskPoolInner { threads, shutdown }),
        })
    }

    /// Returns the number of threads in the pool.
    #[inline]
    pub fn thread_count(&self) -> usize {
        self.inner.threads.len()
    }

    /// Spawns a [`Task`] in the thread pool.
    #[inline]
    pub fn spawn<T>(&self, future: impl Future<Output = T> + Send + 'static) -> Task<T>
    where
        T: Send + 'static,
    {
        Task::new(self.executor.spawn(future))
    }

    /// Spawns a [`Task`] in the thread local executor.
    #[inline]
    pub fn spawn_local<T>(&self, future: impl Future<Output = T> + Send + 'static) -> Task<T>
    where
        T: Send + 'static,
    {
        Task::new(Self::LOCAL_EXECUTOR.with(|executor| executor.spawn(future)))
    }
}
