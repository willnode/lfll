use core::marker::PhantomData;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

const MARK_BIT: usize = 1;
const FLAG_BIT: usize = 2;
const PTR_MASK: usize = !(MARK_BIT | FLAG_BIT);

/// Successor data for linked list node.
/// Mostly a helper to pack into single atomic pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuccData<T> {
    /// This Node in pointer, not Arc.
    /// It's may tempting replace everything in this codebase with Rc, but please no.
    pub ptr: *mut T,
    /// Control when the right pointer of this node can be changed anytime.
    /// When it does, backlinks should be instead until the node is not marked.
    pub mark: bool,
    /// Control when this node is marked for deletion.
    /// When it does, deletion is happening and pointer cannot be changed (mark must false).
    pub flag: bool,
}

impl<T> SuccData<T> {
    pub fn new(ptr: *mut T, mark: bool, flag: bool) -> Self {
        Self { ptr, mark, flag }
    }

    fn into_packed(self) -> usize {
        let mut val = (self.ptr as usize) & PTR_MASK;
        if self.mark {
            val |= MARK_BIT;
        }
        if self.flag {
            val |= FLAG_BIT;
        }
        val
    }

    fn from_packed(val: usize) -> Self {
        Self {
            ptr: (val & PTR_MASK) as *mut T,
            mark: (val & MARK_BIT) != 0,
            flag: (val & FLAG_BIT) != 0,
        }
    }
}

/// Structure that holds SuccData inside an AtomicUsize
pub struct AtomicSucc<T> {
    inner: AtomicUsize,
    _marker: PhantomData<*mut T>,
}

impl<T> AtomicSucc<T> {
    pub fn new(initial: SuccData<T>) -> Self {
        Self {
            inner: AtomicUsize::new(initial.into_packed()),
            _marker: PhantomData,
        }
    }

    pub fn load(&self, order: Ordering) -> SuccData<T> {
        SuccData::from_packed(self.inner.load(order))
    }

    pub fn store(&self, data: SuccData<T>, order: Ordering) {
        self.inner.store(data.into_packed(), order);
    }

    pub fn compare_exchange(
        &self,
        expected: SuccData<T>,
        new: SuccData<T>,
        success: Ordering,
        failure: Ordering,
    ) -> Result<SuccData<T>, SuccData<T>> {
        match self.inner.compare_exchange(
            expected.into_packed(),
            new.into_packed(),
            success,
            failure,
        ) {
            Ok(val) => Ok(SuccData::from_packed(val)),
            Err(val) => Err(SuccData::from_packed(val)),
        }
    }
}

impl<T> Default for AtomicSucc<T> {
    fn default() -> Self {
        Self::new(SuccData::new(core::ptr::null_mut(), false, false))
    }
}
