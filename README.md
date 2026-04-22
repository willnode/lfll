# Lock-Free Linked List Algorithms

Based on 2003 paper by [Mikhail Fomitchev and Eric Ruppert](https://www.eecs.yorku.ca/~eruppert/papers/lfll.pdf), written in Rust.

Data types:

+ `LockFreeLinkedList` minimal implementation of ordered linked list
+ `LockFreeSkipList` ordered linked list with hash table
+ `LockFreeDequeList` LIFO/FIFO non-ordered linked list

Benchmark as of [1964b54](https://github.com/willnode/lfll/commit/1964b545ec2e90b328121073cd8a44b871f68b94) running from [crates/bench](./crates/bench/) on Virtualized Linux on top of Mac M4:

| Data Structure     | R80% / W10% / D10%(50 Keys) | R70% / W20% / D10%(1,000 Keys) | R50% / W50% / D0%(10,000 Keys) | R0% / W60% / D40%(100,000 Keys) |
|--------------------|-----------------------------|--------------------------------|--------------------------------|---------------------------------|
| LockFreeLinkedList | 862.01 µs                   | 6.0297 ms                      | 120.24 ms                      | 280.98 ms                       |
| LockFreeSkipList   | 1.5449 ms                   | 5.4119 ms                      | 95.564 ms                      | 86.065 ms                       |
| LockFreeDequeList  | **611.37 µs**               | **2.7976 ms**                  | 38.924 ms                      | 81.102 ms                       |
| Mutex\<Vec\>       | 1.8324 ms                   | 7.4807 ms                      | 10.366 ms                      | 76.339 ms                       |
| Mutex\<BTreeMap\>  | 1.7751 ms                   | 3.1569 ms                      | **5.1027 ms**                  | **7.3387 ms**                   |
| RwLock\<BTreeMap\> | 2.6591 ms                   | 4.2370 ms                      | 6.3475 ms                      | 8.3787 ms                       |
