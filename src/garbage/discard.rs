#[cfg(feature = "std")]
use std::sync::{Arc, Mutex};

use crate::{GarbageCollector, ScopedGarbageCollector};

pub struct DiscardedGC;

impl<N> GarbageCollector<N> for DiscardedGC {
    fn push(_del: *mut N) {
        // yes, it does nothing and leak memory
    }
}

impl ScopedGarbageCollector for DiscardedGC {
    #[cfg(feature = "std")]
    fn new() -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {}))
    }

    #[cfg(feature = "std")]
    fn collect(&mut self) {}

    unsafe fn prune_now() -> usize {
        0
    }

    fn prune_all() -> usize {
        0
    }

    #[cfg(feature = "std")]
    fn prune(&mut self) -> usize {
        0
    }
}
