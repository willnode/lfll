use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use rand::Rng;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use lfll::{List, LockFreeDequeList, LockFreeLinkedList, LockFreeSkipList};

#[derive(Clone, Copy)]
enum Op {
    Contains(i32),
    Insert(i32),
    Delete(i32),
}

struct BenchConfig {
    read_percent: u8,
    insert_percent: u8,
    delete_percent: u8,
    max_key: i32,
}

/// Generates a randomized workload with RNG
fn generate_workload(
    num_ops: usize,
    read_percent: u8,
    insert_percent: u8,
    max_key: i32,
) -> Vec<Op> {
    let mut rng = rand::thread_rng();
    let mut ops = Vec::with_capacity(num_ops);

    for _ in 0..num_ops {
        let key = rng.gen_range(1..=max_key);
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
    let mut group = c.benchmark_group("Concurrent DS");

    group.sample_size(50);

    let num_threads = 8;
    let ops_per_thread = 5_000;

    let scenarios = vec![
        BenchConfig {
            read_percent: 80,
            insert_percent: 10,
            delete_percent: 10,
            max_key: 50,
        },
        BenchConfig {
            read_percent: 70,
            insert_percent: 20,
            delete_percent: 10,
            max_key: 1000,
        },
        BenchConfig {
            read_percent: 50,
            insert_percent: 50,
            delete_percent: 0,
            max_key: 10000,
        },
        BenchConfig {
            read_percent: 0,
            insert_percent: 60,
            delete_percent: 40,
            max_key: 100000,
        },
    ];

    for config in scenarios {
        let scenario_id = format!(
            "R{}%|W{}%|D{}%|Keys{}",
            config.read_percent, config.insert_percent, config.delete_percent, config.max_key
        );

        let thread_workloads: Vec<Vec<Op>> = (0..num_threads)
            .map(|_| {
                generate_workload(
                    ops_per_thread,
                    config.read_percent,
                    config.insert_percent,
                    config.max_key,
                )
            })
            .collect();

        group.bench_with_input(
            BenchmarkId::new("LockFreeLinkedList", &scenario_id),
            &thread_workloads,
            |b, workloads| {
                b.iter(|| {
                    let list = Arc::new(LockFreeLinkedList::<i32, i32>::new());
                    thread::scope(|s| {
                        for workload in workloads {
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
            },
        );

        group.bench_with_input(
            BenchmarkId::new("LockFreeSkipList", &scenario_id),
            &thread_workloads,
            |b, workloads| {
                b.iter(|| {
                    let list = Arc::new(LockFreeSkipList::<i32, i32>::new());
                    thread::scope(|s| {
                        for workload in workloads {
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
            },
        );

        group.bench_with_input(
            BenchmarkId::new("LockFreeDequeList", &scenario_id),
            &thread_workloads,
            |b, workloads| {
                b.iter(|| {
                    let list = Arc::new(LockFreeDequeList::<i64>::new());
                    thread::scope(|s| {
                        for workload in workloads {
                            let list_ref = &list;
                            s.spawn(move || {
                                for &op in workload {
                                    match op {
                                        Op::Contains(k) => {
                                            black_box(list_ref.contains(&k.into()));
                                        }
                                        Op::Insert(k) => {
                                            black_box(list_ref.push_back(k.into()));
                                        }
                                        Op::Delete(k) => {
                                            black_box(list_ref.delete(&k.into()));
                                        }
                                    }
                                }
                            });
                        }
                    });
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("LockFreeDequeListFront", &scenario_id),
            &thread_workloads,
            |b, workloads| {
                b.iter(|| {
                    let list = Arc::new(LockFreeDequeList::<i64>::new());
                    thread::scope(|s| {
                        for workload in workloads {
                            let list_ref = &list;
                            s.spawn(move || {
                                for &op in workload {
                                    match op {
                                        Op::Contains(k) => {
                                            black_box(list_ref.contains(&k.into()));
                                        }
                                        Op::Insert(k) => {
                                            black_box(list_ref.push_front(k.into()));
                                        }
                                        Op::Delete(k) => {
                                            black_box(list_ref.delete(&k.into()));
                                        }
                                    }
                                }
                            });
                        }
                    });
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("Mutex<Vec>", &scenario_id),
            &thread_workloads,
            |b, workloads| {
                b.iter(|| {
                    let map = Arc::new(Mutex::new(Vec::<i32>::new()));
                    thread::scope(|s| {
                        for workload in workloads {
                            let map_ref = &map;
                            s.spawn(move || {
                                for &op in workload {
                                    match op {
                                        Op::Contains(k) => {
                                            let lock = map_ref.lock().unwrap();
                                            black_box(lock.contains(&k));
                                        }
                                        Op::Insert(k) => {
                                            let mut lock = map_ref.lock().unwrap();
                                            black_box(lock.push(k));
                                        }
                                        Op::Delete(k) => {
                                            let mut lock = map_ref.lock().unwrap();
                                            if let Some(pos) = lock.iter().position(|x| *x == k) {
                                                black_box(lock.remove(pos));
                                            }
                                        }
                                    }
                                }
                            });
                        }
                    });
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("Mutex<BTreeMap>", &scenario_id),
            &thread_workloads,
            |b, workloads| {
                b.iter(|| {
                    let map = Arc::new(Mutex::new(BTreeMap::<i32, i32>::new()));
                    thread::scope(|s| {
                        for workload in workloads {
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
            },
        );

        group.bench_with_input(
            BenchmarkId::new("RwLock<BTreeMap>", &scenario_id),
            &thread_workloads,
            |b, workloads| {
                b.iter(|| {
                    let map = Arc::new(RwLock::new(BTreeMap::<i32, i32>::new()));
                    thread::scope(|s| {
                        for workload in workloads {
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
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_concurrent_ds);
criterion_main!(benches);
