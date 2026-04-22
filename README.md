# Lock-Free Linked List Algorithms

Based on 2003 paper by [Mikhail Fomitchev and Eric Ruppert](https://www.eecs.yorku.ca/~eruppert/papers/lfll.pdf), written in Rust.

Data types:

+ `LockFreeLinkedList` minimal implementation of ordered linked list
+ `LockFreeSkipList` ordered linked list with hash table
+ `LockFreeDequeList` LIFO/FIFO non-ordered linked list

Benchmark as of [59e354a](https://github.com/willnode/lfll/commit/59e354a9ea3ec0acf4216ef9c3d19267425d8f8a) running from [crates/bench](./crates/bench/) on Virtualized Linux on top of Mac M4 (lower is better):

| Data Structure         | R70% / W20% / D10%(50 Keys) | R50% / W50% / D0%(2,000 Keys) | R0% / W100% / D0%(20,000 Keys) | R0% / W50% / D50%(20,000 Keys) |
|------------------------|-----------------------------|-------------------------------|--------------------------------|--------------------------------|
| LockFreeLinkedList     | 1.0879 ms                   | 19.450 ms                     | 520.53 ms                      | 204.84 ms                      |
| LockFreeSkipList       | 2.7720 ms                   | 19.706 ms                     | 682.71 ms                      | 32.979 ms                      |
| LockFreeDequeList      | 1.5829 ms                   | 16.517 ms                     | 10.905 ms                      | 86.231 ms                      |
| LockFreeDequeListFront | 54.618 ms                   | 161.15 ms                     | 9.5152 ms                      | 179.31 ms                      |
| Mutex\<Vec\>             | 3.7469 ms                   | 8.9529 ms                     | 961.27 µs                      | 140.24 ms                      |
| Mutex\<BTreeMap\>        | 1.9678 ms                   | 8.9093 ms                     | 14.999 ms                      | 12.034 ms                      |
| RwLock\<BTreeMap\>       | 3.2602 ms                   | 11.925 ms                     | 17.122 ms                      | 12.722 ms                      |

In summary: 
+ `LockFreeLinkedList` is faster in insertion than `LockFreeSkipList` but the latter is scalable. Use either if you want them ordered
+ `LockFreeDequeList` is fast to push new items at the end of the linked list, while pushing in front can introduce contention at searching
+ `LockFreeSkipList` has similar searching and deletoin performance to `BTreeMap` while insertion is worse because the latter is using amortized insert
+ All `LockFree` delete performance has the same performance with searching, either using `LockFreeSkipList` or remembering the node pointer to save the `O(N)` time it takes would help in practical speed.
