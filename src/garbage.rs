#[cfg(feature = "std")]
use std::sync::{Arc, Mutex};

#[cfg(feature = "std")]
mod threaded;
#[cfg(feature = "std")]
pub use threaded::*;

mod discard;
pub use discard::*;

#[cfg(feature = "std")]
pub type DefaultGC = ThreadedGC;

#[cfg(not(feature = "std"))]
pub type DefaultGC = DiscardedGC;
#[cfg(not(feature = "std"))]
use alloc::boxed::Box;

pub trait GarbageCollector<N>
where
    N: Sized,
{
    /// Add garbage into the global collector
    fn push(del: *mut N);

    /// Internal drop function
    unsafe fn dropper(ptr: *mut u8) {
        let typed_ptr = ptr as *mut N;
        let _ = unsafe { Box::from_raw(typed_ptr) };
    }
}

pub trait ScopedGarbageCollector {
    /// Create new scoped GC
    #[cfg(feature = "std")]
    fn new() -> Arc<Mutex<Self>>;
    /// Collect garbage from the local collector
    #[cfg(feature = "std")]
    fn collect(&mut self);
    /// Drop garbage from the local collector.
    /// Unsafe because other thread maybe accessing
    unsafe fn prune_now() -> usize;
    /// Drop the global collector garbage.
    /// This is always safe because garbage are moved to global then thread destroyed.
    /// Note that current thread will not be part of pruning.
    fn prune_all() -> usize;
    /// Drop collected garbage
    #[cfg(feature = "std")]
    fn prune(&mut self) -> usize;
}

#[derive(Clone)]
pub struct GarbageItem {
    #[cfg(feature = "std")]
    ptr: *mut u8,
    #[cfg(feature = "std")]
    dropper: unsafe fn(*mut u8),
}

unsafe impl Send for GarbageItem {}
