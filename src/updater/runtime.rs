use std::future::Future;
use std::sync::LazyLock;

use tokio::runtime::{Builder, Runtime};

static TOKIO_RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("textree-net")
        .enable_all()
        .build()
        .expect("failed to initialize Tokio runtime")
});

pub fn spawn_tokio<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    TOKIO_RUNTIME.spawn(future)
}
