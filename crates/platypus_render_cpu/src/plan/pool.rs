// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A few threads that stay up and run row bands: a step hands out `count` band indices and returns
//! when all are done (the calling thread takes bands too). Spawning threads per step cost about a
//! millisecond each time (24 threads, measured); these wait on a condition variable between frames and
//! cost nothing then.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

/// One job: run `f` for each index below `count`, claimed through `next`.
#[derive(Clone, Copy)]
struct Job {
    f: *const (dyn Fn(usize) + Sync),
    next: *const AtomicUsize,
    count: usize,
}

// SAFETY: the pointers are to a closure and a counter on the stack of `Pool::run`, which waits for
// every worker to finish with them before it returns.
unsafe impl Send for Job {}

#[derive(Default)]
struct State {
    job: Option<Job>,
    generation: u64,
    /// Workers still on the current job.
    busy: usize,
    quit: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    done: Condvar,
}

pub struct Pool {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
    /// Held while a job runs: the workers take one job at a time, so a second caller (two renderers drawing on
    /// two threads at once) draws its bands itself instead of replacing the first caller's job.
    running: std::sync::Mutex<()>,
}

/// Most threads a process rasterises with, the caller included (llvmpipe's budget in Fluid's apps).
const MOST_THREADS: usize = 4;

/// How many threads the process's pool has: `PLATYPUS_CPU_THREADS` if set, else the machine's cores
/// up to [`MOST_THREADS`]. A one-core machine gets no worker at all, a two-core one a single worker.
fn thread_count() -> usize {
    if let Some(n) = std::env::var("PLATYPUS_CPU_THREADS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        return n.clamp(1, 64);
    }
    std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(MOST_THREADS)
}

/// The process's one pool, made the first time a frame needs it and shared by every renderer in
/// the process: an app with several windows still has one set of threads.
pub fn shared() -> &'static Pool {
    static POOL: std::sync::OnceLock<Pool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| Pool::new(thread_count()))
}

impl Pool {
    /// A pool of `threads` threads in all (the caller counts as one).
    pub fn new(threads: usize) -> Pool {
        let shared = Arc::new(Shared::default());
        let workers = (1..threads.max(1))
            .map(|i| {
                let shared = shared.clone();
                std::thread::Builder::new()
                    .name(format!("cpu-raster-{i}"))
                    // Kernels keep their data on the heap: a small stack is plenty.
                    .stack_size(256 * 1024)
                    .spawn(move || work(&shared))
                    .expect("spawning a raster thread")
            })
            .collect();
        Pool {
            shared,
            workers,
            running: std::sync::Mutex::new(()),
        }
    }

    /// Threads that run bands, the caller included.
    pub fn threads(&self) -> usize {
        self.workers.len() + 1
    }

    /// Runs `f(i)` for every `i < count`, spread over the pool, and returns when all have run. While another thread's
    /// job runs, this one runs on the calling thread alone.
    pub fn run(&self, count: usize, f: &(dyn Fn(usize) + Sync)) {
        if self.workers.is_empty() || count <= 1 {
            (0..count).for_each(f);
            return;
        }
        let Ok(_running) = self.running.try_lock() else {
            (0..count).for_each(f);
            return;
        };
        let next = AtomicUsize::new(0);
        // SAFETY: only to erase the lifetime; `f` outlives the job, as this waits for it below.
        let f: *const (dyn Fn(usize) + Sync) = unsafe { std::mem::transmute(f) };
        {
            let mut s = self.shared.state.lock().expect("pool state");
            s.job = Some(Job {
                f,
                next: &next,
                count,
            });
            s.generation += 1;
            s.busy = self.workers.len();
        }
        self.shared.wake.notify_all();
        claim(f, &next, count);
        let mut s = self.shared.state.lock().expect("pool state");
        while s.busy > 0 {
            s = self.shared.done.wait(s).expect("pool state");
        }
        s.job = None;
    }
}

/// Runs the indices left in `next`.
fn claim(f: *const (dyn Fn(usize) + Sync), next: &AtomicUsize, count: usize) {
    loop {
        let i = next.fetch_add(1, Ordering::Relaxed);
        if i >= count {
            break;
        }
        // SAFETY: `Pool::run` keeps the closure alive until every worker is done.
        unsafe { (*f)(i) };
    }
}

fn work(shared: &Shared) {
    let mut seen = 0;
    loop {
        let job = {
            let mut s = shared.state.lock().expect("pool state");
            while s.generation == seen && !s.quit {
                s = shared.wake.wait(s).expect("pool state");
            }
            if s.quit {
                return;
            }
            seen = s.generation;
            s.job
        };
        if let Some(job) = job {
            // SAFETY: the counter lives until `run` returns, which waits for this worker.
            claim(job.f, unsafe { &*job.next }, job.count);
        }
        let mut s = shared.state.lock().expect("pool state");
        s.busy -= 1;
        if s.busy == 0 {
            shared.done.notify_one();
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.state.lock().expect("pool state").quit = true;
        self.shared.wake.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_band_runs_once_however_many_threads() {
        for threads in [1, 2, 4] {
            let pool = Pool::new(threads);
            for _ in 0..50 {
                let hits: Vec<AtomicUsize> = (0..37).map(|_| AtomicUsize::new(0)).collect();
                pool.run(hits.len(), &|i| {
                    hits[i].fetch_add(1, Ordering::Relaxed);
                });
                assert!(hits.iter().all(|h| h.load(Ordering::Relaxed) == 1));
            }
        }
    }

    // M2-KRN-13
    #[test]
    fn two_threads_running_jobs_at_once_both_finish_with_every_band_once() {
        let pool = Arc::new(Pool::new(4));
        let (done, finished) = std::sync::mpsc::channel();
        for _ in 0..2 {
            let (pool, done) = (pool.clone(), done.clone());
            std::thread::spawn(move || {
                for _ in 0..200 {
                    let hits: Vec<AtomicUsize> = (0..37).map(|_| AtomicUsize::new(0)).collect();
                    pool.run(hits.len(), &|i| {
                        hits[i].fetch_add(1, Ordering::Relaxed);
                    });
                    assert!(hits.iter().all(|h| h.load(Ordering::Relaxed) == 1));
                }
                done.send(()).unwrap();
            });
        }
        // Before the pool took one job at a time, the second caller replaced the first's job and both waited for ever.
        for _ in 0..2 {
            finished
                .recv_timeout(std::time::Duration::from_secs(30))
                .expect("both callers finish");
        }
    }
}
