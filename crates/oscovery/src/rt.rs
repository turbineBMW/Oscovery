//! One shared tokio runtime for the network and for child processes; the
//! GTK thread hands it futures and gets answers back over a channel.
//!
//! The shape of every call: `rt::run` the work, and only `on_done`, on the
//! main loop, touches widgets or the database.

use std::future::Future;
use std::sync::OnceLock;

pub fn handle() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("oscovery-io")
            .build()
            .expect("tokio runtime")
    })
}

pub fn spawn<F: Future<Output = ()> + Send + 'static>(future: F) {
    handle().spawn(future);
}

/// Run `job` on the runtime and hand its result to `on_done` on the main
/// loop. The GTK side's one way to do I/O.
pub fn run<T, F, D>(job: F, on_done: D)
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
    D: FnOnce(T) + 'static,
{
    let (tx, rx) = async_channel::bounded::<T>(1);
    spawn(async move {
        let _ = tx.send(job.await).await;
    });
    glib::spawn_future_local(async move {
        if let Ok(result) = rx.recv().await {
            on_done(result);
        }
    });
}
