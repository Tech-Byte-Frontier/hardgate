use super::*;

fn sample(total: u64, available: u64) -> memory::MemorySample {
    memory::MemorySample {
        total_bytes: total * MIB,
        available_bytes: available * MIB,
        full_avg10: 0.0,
        some_avg10: 0.0,
    }
}

#[test]
fn worker_memory_limits_concurrency_before_cpu_capacity() {
    assert_eq!(plan(&sample(8192, 6500), 8, 3072 * MIB).unwrap().jobs, 1);
    assert_eq!(plan(&sample(16384, 13000), 8, 3072 * MIB).unwrap().jobs, 3);
    assert_eq!(plan(&sample(16384, 13000), 2, 3072 * MIB).unwrap().jobs, 2);
    assert_eq!(plan(&sample(8192, 6500), 8, 2048 * MIB).unwrap().jobs, 2);
}

#[test]
fn insufficient_capacity_never_forces_one_worker() {
    assert!(plan(&sample(4096, 3500), 8, 3072 * MIB).is_err());
    assert!(plan(&sample(8192, 500), 8, 1024 * MIB).is_err());
    assert!(plan(&sample(8192, 6500), 8, 0).is_err());
    let mut pressured = sample(8192, 6500);
    pressured.full_avg10 = 1.0;
    assert!(plan(&pressured, 8, 1024 * MIB).is_err());
}
