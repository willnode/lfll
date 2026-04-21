// Lock-Free Linked Lists Algorithm from https://www.eecs.yorku.ca/~eruppert/papers/lfll.pdf
// https://web.archive.org/web/20040825221013/http://www.cs.yorku.ca/~mikhail/MSc.Thesis.pdf

mod linked_list;
pub use linked_list::*;
pub(crate) mod succ;
