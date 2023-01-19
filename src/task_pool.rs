use std::{future::Future, io, marker::PhantomData, mem, sync::Arc, thread::JoinHandle};

use async_executor::{Executor, LocalExecutor};
use async_task::FallibleTask;
use concurrent_queue::ConcurrentQueue;
use event_listener::Event;
use futures_lite::{future, pin};
use once_cell::sync::OnceCell;

use crate::Task;

/// Builder for a [`TaskPool`].
#[must_use = "TaskPoolBuilder does nothing unless you call `build`"]
#[derive(Clone, Debug, Default)]
pub struct TaskPoolBuilder {
    num_threads: Option<usize>,
    stack_size: Option<usize>,
    thread_name: Option<String>,
}

impl TaskPoolBuilder {
    /// Creates a new [`TaskPoolBuilder`].
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

    /// Builds the [`TaskPool`].
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
    shutdown: Event,
}

impl Drop for TaskPoolInner {
    fn drop(&mut self) {
        self.shutdown.notify(usize::MAX);

        let panicking = std::thread::panicking();
        for thread in self.threads.drain(..) {
            let result = thread.join();

            if !panicking {
                result.expect("Task thread panicked.");
            }
        }
    }
}

/// A pool of threads for running [`Task`]s.
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

    /// Gets a reference to the global [`TaskPool`].
    ///
    /// # Panics
    /// Panics if the global [`TaskPool`] fails initialization, this should only happen if spawning
    /// the thread pool fails.
    pub fn global() -> &'static Self {
        static GLOBAL_POOL: OnceCell<TaskPool> = OnceCell::new();
        GLOBAL_POOL.get_or_init(|| TaskPool::new().expect("Failed to create global task pool."))
    }

    /// Creates a new [`TaskPoolBuilder`].
    ///
    /// # Examples
    /// ```rust
    /// # use hyena::TaskPool;
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
        let shutdown = Event::new();

        let executor = Arc::new(Executor::new());

        let num_threads = num_threads.unwrap_or_else(num_cpus::get);

        let threads: Vec<_> = (0..num_threads)
            .map(|i| {
                let executor = executor.clone();
                let shutdown = shutdown.listen();
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
                    let fut = executor.run(shutdown);
                    future::block_on(fut);
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
    pub fn spawn_local<T>(&self, future: impl Future<Output = T> + 'static) -> Task<T>
    where
        T: 'static,
    {
        Task::new(Self::LOCAL_EXECUTOR.with(|executor| executor.spawn(future)))
    }

    /// Allows spawning futures on the thread pool that aren't `'static`. The function takes a
    /// callback passing a [`Scope`] to it. The [`Scope`] can be used to spawn futures. This
    /// function will wait all futures spawned on the [`Scope`] to completion before returning.
    #[inline]
    pub fn scope<'env, F, T>(&self, f: F) -> Vec<T>
    where
        F: for<'scope> FnOnce(&'scope Scope<'scope, 'env, T>),
        T: Send + 'static,
    {
        // SAFETY: This safety comment applies to all references transmuted to 'env.
        // Any futures spawned with these references need to return before this function completes.
        // This is guaranteed because we drive all the futures spawned onto the Scope
        // to completion in this function. However, rust has no way of knowing this so we
        // transmute the lifetimes to 'env here to appease the compiler as it is unable to validate safety.
        let executor: &async_executor::Executor = &self.executor;
        let executor: &'env async_executor::Executor = unsafe { mem::transmute(executor) };
        let task_scope_executor = &async_executor::Executor::default();
        let task_scope_executor: &'env async_executor::Executor =
            unsafe { mem::transmute(task_scope_executor) };
        let spawned: ConcurrentQueue<FallibleTask<T>> = ConcurrentQueue::unbounded();
        let spawned_ref: &'env ConcurrentQueue<FallibleTask<T>> =
            unsafe { mem::transmute(&spawned) };

        let scope = Scope {
            executor,
            task_scope_executor,
            spawned: spawned_ref,
            scope: PhantomData,
            env: PhantomData,
        };

        let scope_ref: &'env Scope<'_, 'env, T> = unsafe { mem::transmute(&scope) };

        f(scope_ref);

        if spawned.is_empty() {
            Vec::new()
        } else {
            let get_results = async {
                let mut results = Vec::with_capacity(spawned_ref.len());
                while let Ok(task) = spawned_ref.pop() {
                    results.push(task.await.unwrap());
                }

                results
            };

            // Pin the futures on the stack.
            pin!(get_results);

            loop {
                if let Some(result) = future::block_on(future::poll_once(&mut get_results)) {
                    break result;
                };

                std::panic::catch_unwind(|| {
                    executor.try_tick();
                    task_scope_executor.try_tick();
                })
                .ok();
            }
        }
    }
}

/// Allow spawning [`Task`]s on the thread pool that aren't `'static`.
///
/// For more information, see [`TaskPool::scope`].
#[derive(Debug)]
pub struct Scope<'scope, 'env: 'scope, T> {
    executor: &'scope Executor<'scope>,
    task_scope_executor: &'scope Executor<'scope>,
    spawned: &'scope ConcurrentQueue<FallibleTask<T>>,
    scope: PhantomData<&'scope mut &'scope ()>,
    env: PhantomData<&'env mut &'env ()>,
}

impl<'scope, 'env, T> Scope<'scope, 'env, T>
where
    T: Send + 'scope,
{
    /// Spawns a future onto the thread pool.
    #[inline]
    pub fn spawn(&self, future: impl Future<Output = T> + Send + 'scope) {
        let task = self.executor.spawn(future).fallible();

        self.spawned.push(task).unwrap();
    }

    /// Spawns a future onto the thread the scope is run on.
    #[inline]
    pub fn spawn_on_scope(&self, future: impl Future<Output = T> + Send + 'scope) {
        let task = self.task_scope_executor.spawn(future).fallible();

        self.spawned.push(task).unwrap();
    }
}
