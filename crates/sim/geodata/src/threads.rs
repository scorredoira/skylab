//! Threads for the work of a region: std's natively. On the web std cannot start a thread, and a
//! worker (where this crate loads: `engine::threads`) cannot start another while it works, so
//! `scope` runs its jobs one after another there: a region is built on one worker, its tiles
//! downloaded one at a time.

#[cfg(not(target_arch = "wasm32"))]
pub use std::thread::scope;

#[cfg(target_arch = "wasm32")]
pub use inline::scope;

/// `std::thread::scope` with the jobs run one after another as they are spawned.
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
