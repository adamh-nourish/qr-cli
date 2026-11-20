//! The batch worker threads.
//!
//! Only `qr batch` starts threads, through [`build`], which makes a local
//! rayon pool (never rayon's global one) and counts every worker it
//! starts. The one-shot commands must leave [`threads_started`] at zero
//! (DECISIONS section 5); `tests/one_shot.rs` asserts that.

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::{ThreadPool, ThreadPoolBuilder};

use super::BatchError;

/// The most worker threads `--jobs` accepts.
pub const MAX_JOBS: u16 = 256;

/// Worker threads started by [`build`] in this process.
static THREADS_STARTED: AtomicUsize = AtomicUsize::new(0);

/// How many batch worker threads this process has started. Zero unless a
/// `qr batch` has run: the one-shot path never starts a thread pool.
pub fn threads_started() -> usize {
    THREADS_STARTED.load(Ordering::Relaxed)
}

/// The default `--jobs`: the parallelism the system reports, or 1.
pub(crate) fn default_jobs() -> usize {
    std::thread::available_parallelism().map_or(1, NonZeroUsize::get)
}

/// A pool of `jobs` worker threads for one batch.
///
/// # Errors
///
/// [`BatchError::Pool`] if the threads cannot be started.
pub(crate) fn build(jobs: usize) -> Result<ThreadPool, BatchError> {
    ThreadPoolBuilder::new()
        .num_threads(jobs)
        .thread_name(|i| format!("qr-batch-{i}"))
        .start_handler(|_| {
            THREADS_STARTED.fetch_add(1, Ordering::Relaxed);
        })
        .build()
        .map_err(|e| BatchError::Pool {
            jobs,
            message: e.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pools_count_their_threads() {
        let before = threads_started();
        let pool = build(3).unwrap();
        assert_eq!(pool.current_num_threads(), 3);
        let names: Vec<String> =
            pool.broadcast(|_| std::thread::current().name().unwrap_or_default().to_owned());
        assert!(
            names.iter().all(|n| n.starts_with("qr-batch-")),
            "{names:?}"
        );
        assert!(threads_started() >= before + 3);
        assert!(default_jobs() >= 1);
    }
}
