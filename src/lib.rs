//! Lock-Free Linked Lists Algorithm based on 2003 paper by [Mikhail Fomitchev and Eric Ruppert](https://www.eecs.yorku.ca/~eruppert/papers/lfll.pdf).
//! The algorithm provides atomic and ordered linked list mainly using atomic primitives. It is mainly done with pointers and flags to
//! eliminate [ABA problem](https://en.wikipedia.org/wiki/ABA_problem) by using mathematical proofs guaranteed from the paper.
//! Most internal API is exposed by design for extra performance tweaking at your own peril, because those are `*mut` raw pointers, these is mostly unsafe. But accessing safe functions would be enough for standard use-cases.
//!
//! See `LockFreeLinkedList`, `LockFreeSkipList`, `LockFreeDequeList` for more information about the usage.

mod linked_list;
pub use linked_list::*;
pub(crate) mod succ;
pub use succ::{List, Node};

mod skip_list;
pub use skip_list::*;
mod garbage;
pub use garbage::*;
mod seedable;
pub use seedable::*;

mod deque_list;
pub use deque_list::*;
