//! Shared runtime for GUI adapters, the local service, and library tests.
use std::{future::Future, sync::OnceLock};
use tokio::runtime::Runtime;
fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("ck-launcher")
            .build()
            .expect("create launcher runtime")
    })
}
pub fn block_on<F: Future>(future: F) -> F::Output {
    runtime().block_on(future)
}
pub fn spawn<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    runtime().spawn(future)
}
pub fn spawn_blocking<F, R>(function: F) -> tokio::task::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    runtime().spawn_blocking(function)
}
