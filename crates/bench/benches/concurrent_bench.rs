use criterion::{Criterion, black_box, criterion_group, criterion_main};
use rand::Rng;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use lfll::{List, LockFreeLinkedList, LockFreeSkipList};

#[derive(Clone, Copy)]
enum Op {
    Contains(i32),
    Insert(i32),
    Delete(i32),
}

/// Generates a randomized workload with RNG
fn generate_workload(num_ops: usize, read_percent: u8, insert_percent: u8) -> Vec<Op> {
    let mut rng = rand::thread_rng();
    let mut ops = Vec::with_capacity(num_ops);

    for _ in 0..num_ops {
        let key = rng.gen_range(1..1000);
        let roll = rng.gen_range(0..100);

        if roll < read_percent {
            ops.push(Op::Contains(key));
        } else if roll < read_percent + insert_percent {
            ops.push(Op::Insert(key));
        } else {
            ops.push(Op::Delete(key));
        }
    }
    ops
}

fn bench_concurrent_ds(c: &mut Criterion) {
    let mut group = c.benchmark_group("Concurrent Dictionaries");

    let num_threads = 8;
    let ops_per_thread = 5_000;

    let read_ratio = 0;
    let insert_ratio = 50;

    let thread_workloads: Vec<Vec<Op>> = (0..num_threads)
        .map(|_| generate_workload(ops_per_thread, read_ratio, insert_ratio))
        .collect();

    group.bench_function("LockFreeLinkedList", |b| {
        b.iter(|| {
            let list = Arc::new(LockFreeLinkedList::<i32, i32>::new());

            thread::scope(|s| {
                for workload in &thread_workloads {
                    let list_ref = &list;
                    s.spawn(move || {
                        for &op in workload {
                            match op {
                                Op::Contains(k) => {
                                    black_box(list_ref.contains(&k));
                                }
                                Op::Insert(k) => {
                                    black_box(list_ref.insert(k, k));
                                }
                                Op::Delete(k) => {
                                    black_box(list_ref.delete(&k));
                                }
                            }
                        }
                    });
                }
            });
        })
    });

    group.bench_function("LockFreeSkipList", |b| {
        b.iter(|| {
            let list = Arc::new(LockFreeSkipList::<i32, i32>::new());

            thread::scope(|s| {
                for workload in &thread_workloads {
                    let list_ref = &list;
                    s.spawn(move || {
                        for &op in workload {
                            match op {
                                Op::Contains(k) => {
                                    black_box(list_ref.contains(&k));
                                }
                                Op::Insert(k) => {
                                    black_box(list_ref.insert(k, k));
                                }
                                Op::Delete(k) => {
                                    black_box(list_ref.delete(&k));
                                }
                            }
                        }
                    });
                }
            });
        })
    });

    group.bench_function("Mutex<BTreeMap>", |b| {
        b.iter(|| {
            let map = Arc::new(Mutex::new(BTreeMap::<i32, i32>::new()));

            thread::scope(|s| {
                for workload in &thread_workloads {
                    let map_ref = &map;
                    s.spawn(move || {
                        for &op in workload {
                            match op {
                                Op::Contains(k) => {
                                    let lock = map_ref.lock().unwrap();
                                    black_box(lock.contains_key(&k));
                                }
                                Op::Insert(k) => {
                                    let mut lock = map_ref.lock().unwrap();
                                    black_box(lock.insert(k, k));
                                }
                                Op::Delete(k) => {
                                    let mut lock = map_ref.lock().unwrap();
                                    black_box(lock.remove(&k));
                                }
                            }
                        }
                    });
                }
            });
        })
    });

    group.bench_function("RwLock<BTreeMap>", |b| {
        b.iter(|| {
            let map = Arc::new(RwLock::new(BTreeMap::<i32, i32>::new()));

            thread::scope(|s| {
                for workload in &thread_workloads {
                    let map_ref = &map;
                    s.spawn(move || {
                        for &op in workload {
                            match op {
                                Op::Contains(k) => {
                                    let lock = map_ref.read().unwrap();
                                    black_box(lock.contains_key(&k));
                                }
                                Op::Insert(k) => {
                                    let mut lock = map_ref.write().unwrap();
                                    black_box(lock.insert(k, k));
                                }
                                Op::Delete(k) => {
                                    let mut lock = map_ref.write().unwrap();
                                    black_box(lock.remove(&k));
                                }
                            }
                        }
                    });
                }
            });
        })
    });

    group.finish();
}

criterion_group!(benches, bench_concurrent_ds);
criterion_main!(benches);
