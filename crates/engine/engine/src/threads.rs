//! Work off the frame's thread, the same on every platform: natively these are std's threads, with
//! no cost added; on the web (`wasm32`) Web Workers that share the module's memory (the web build
//! turns on wasm's threads: `scripts/build-web.sh`).
//!
//! On the web the page's own thread, the one that draws, may never wait: the browser forbids it, and
//! a wait there (`Atomics.wait`, what a contended lock or a `recv` comes down to) is a trap that
//! ends the game. So there:
//! - `spawn` hands the job to a worker of a pool. The workers are started by the page's thread as
//!   jobs need them (a worker that waits cannot start another: its own events would be needed), and
//!   wait for the next job once one is done. A worker only starts once the page's thread has gone
//!   back to the browser: `start_workers` starts some early, before anything waits for them.
//! - `scope` runs its jobs one after another where it is called, as a worker that waited for others
//!   could leave the pool without any free.
//! - `wait` takes a result from a channel: blocking natively and in a worker, spinning on the page's.
//! - A lock the page's thread shares with a worker is taken with `lock`, which spins there.

use std::sync::mpsc::{Receiver, RecvError, TryRecvError};
use std::sync::{Mutex, MutexGuard, TryLockError};

#[cfg(not(target_arch = "wasm32"))]
pub use std::thread::scope;

/// Starts `work` in the background and forgets it; its result goes back through a channel it owns.
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn(work: impl FnOnce() + Send + 'static) {
    std::thread::spawn(work);
}

#[cfg(target_arch = "wasm32")]
pub use workers::{spawn, start_workers, worker_entry, WORKER_SCRIPT};

#[cfg(target_arch = "wasm32")]
pub use inline::scope;

/// Whether this thread may wait: always natively; on the web only in a worker.
pub fn may_wait() -> bool {
    #[cfg(target_arch = "wasm32")]
    return workers::in_worker();
    #[cfg(not(target_arch = "wasm32"))]
    true
}

/// The next value from `receiver`, waiting for it: `recv`, but spinning where a thread may not wait.
pub fn wait<T>(receiver: &Receiver<T>) -> Result<T, RecvError> {
    if may_wait() {
        return receiver.recv();
    }
    loop {
        match receiver.try_recv() {
            Ok(value) => return Ok(value),
            Err(TryRecvError::Disconnected) => return Err(RecvError),
            Err(TryRecvError::Empty) => std::hint::spin_loop(),
        }
    }
}

/// `mutex.lock()`, spinning where a thread may not wait; a lock poisoned by a panic is taken all the
/// same (what it guards must stay whole across a panic). Hold such locks for a moment only.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    if may_wait() {
        return mutex.lock().unwrap_or_else(|e| e.into_inner());
    }
    loop {
        match mutex.try_lock() {
            Ok(guard) => return guard,
            Err(TryLockError::Poisoned(e)) => return e.into_inner(),
            Err(TryLockError::WouldBlock) => std::hint::spin_loop(),
        }
    }
}

/// The Web Workers behind `spawn`.
#[cfg(target_arch = "wasm32")]
mod workers {
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::{Mutex, OnceLock};
    use wasm_bindgen::prelude::*;

    /// The script every worker runs, next to the page: it loads the module with the page's memory
    /// and calls `worker_entry` (`scripts/build-web.sh` writes it).
    pub const WORKER_SCRIPT: &str = "./worker.js";
    /// The most workers started; more jobs at once wait for one to be free.
    const MOST: usize = 16;

    type Job = Box<dyn FnOnce() + Send>;

    struct Pool {
        sender: Sender<Job>,
        /// Only workers take it: they may wait for it.
        receiver: Mutex<Receiver<Job>>,
        /// Jobs sent and not taken yet.
        queued: AtomicUsize,
        /// Workers waiting for a job.
        idle: AtomicUsize,
    }

    fn pool() -> &'static Pool {
        static POOL: OnceLock<Pool> = OnceLock::new();
        POOL.get_or_init(|| {
            let (sender, receiver) = channel();
            Pool { sender, receiver: Mutex::new(receiver), queued: AtomicUsize::new(0), idle: AtomicUsize::new(0) }
        })
    }

    thread_local! {
        static IN_WORKER: Cell<bool> = const { Cell::new(false) };
        /// The workers the page's thread started, kept so that the browser keeps them.
        static STARTED: RefCell<Vec<web_sys::Worker>> = const { RefCell::new(Vec::new()) };
    }

    pub fn in_worker() -> bool {
        IN_WORKER.get()
    }

    /// Hands `work` to a free worker, starting one if none is (on the page's thread).
    pub fn spawn(work: impl FnOnce() + Send + 'static) {
        let pool = pool();
        // The pool is made on the page's thread before any worker exists, so no worker waits for it here.
        let queued = pool.queued.fetch_add(1, Ordering::SeqCst) + 1;
        pool.sender.send(Box::new(work)).expect("the pool's receiver lives as long as the program");
        if !in_worker() && queued > pool.idle.load(Ordering::SeqCst) {
            start_workers(STARTED.with_borrow(Vec::len) + 1);
        }
    }

    /// Starts workers until there are `count` (at most `MOST`), on the page's thread. A worker
    /// only starts once that thread has gone back to the browser, so a job the page's thread will
    /// wait for (`wait`) must find a worker started before: this starts them early.
    pub fn start_workers(count: usize) {
        pool();
        STARTED.with_borrow_mut(|started| {
            while started.len() < count.min(MOST) {
                match start_worker() {
                    Ok(worker) => started.push(worker),
                    Err(error) => {
                        web_sys::console::error_2(&"could not start a worker:".into(), &error);
                        return;
                    }
                }
            }
        });
    }

    fn start_worker() -> Result<web_sys::Worker, JsValue> {
        let options = web_sys::WorkerOptions::new();
        options.set_type(web_sys::WorkerType::Module);
        let worker = web_sys::Worker::new_with_options(WORKER_SCRIPT, &options)?;
        worker.post_message(&js_sys::Array::of2(&wasm_bindgen::module(), &wasm_bindgen::memory()))?;
        Ok(worker)
    }

    /// What a worker runs once the module is loaded in it: jobs, one after another, for ever.
    #[wasm_bindgen]
    pub fn worker_entry() {
        IN_WORKER.set(true);
        let pool = pool();
        loop {
            pool.idle.fetch_add(1, Ordering::SeqCst);
            let job = pool.receiver.lock().unwrap_or_else(|e| e.into_inner()).recv();
            pool.idle.fetch_sub(1, Ordering::SeqCst);
            let Ok(job) = job else { return };
            pool.queued.fetch_sub(1, Ordering::SeqCst);
            job();
        }
    }
}

/// `std::thread::scope` with the jobs run one after another as they are spawned (see the module).
#[cfg(target_arch = "wasm32")]
mod inline {
    use std::marker::PhantomData;

    pub struct Scope<'scope, 'env: 'scope> {
        lifetimes: PhantomData<(&'scope mut &'scope (), &'env mut &'env ())>,
    }

    impl<'scope, 'env> Scope<'scope, 'env> {
        pub fn spawn<T: Send + 'scope>(&'scope self, work: impl FnOnce() -> T + Send + 'scope) {
            work();
        }
    }

    pub fn scope<'env, T>(f: impl for<'scope> FnOnce(&'scope Scope<'scope, 'env>) -> T) -> T {
        f(&Scope { lifetimes: PhantomData })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;
    use std::sync::Mutex;

    #[test]
    fn a_job_hands_back_its_result_and_a_scope_waits_for_all_of_its_own() {
        let (sender, receiver) = channel();
        super::spawn(move || sender.send(6 * 7).unwrap());
        assert_eq!(super::wait(&receiver).unwrap(), 42);
        let mut parts = [0u32; 8];
        super::scope(|scope| {
            for (i, part) in parts.iter_mut().enumerate() {
                scope.spawn(move || *part = i as u32 * 2);
            }
        });
        assert_eq!(parts, [0, 2, 4, 6, 8, 10, 12, 14]);
        let shared = Mutex::new(1);
        *super::lock(&shared) += 1;
        assert_eq!(*super::lock(&shared), 2);
    }
}
