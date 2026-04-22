use std::sync::{Arc, Mutex};

mod threaded;

pub use threaded::*;

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
    fn new() -> Arc<Mutex<Self>>;
    /// Collect garbage from the local collector
    fn collect(&mut self);
    /// Drop garbage from the local collector.
    /// Unsafe because other thread maybe accessing
    unsafe fn prune_now() -> usize;
    /// Drop the global collector garbage.
    /// This is always safe because garbage are moved to global then thread destroyed.
    /// Note that current thread will not be part of pruning.
    fn prune_all() -> usize;
    /// Drop collected garbage
    fn prune(&mut self) -> usize;
}

#[derive(Clone)]
pub struct GarbageItem {
    ptr: *mut u8,
    dropper: unsafe fn(*mut u8),
}

unsafe impl Send for GarbageItem {}
