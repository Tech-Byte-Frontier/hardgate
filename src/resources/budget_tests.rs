use super::*;

fn sample(total: u64, available: u64) -> MemorySample {
    MemorySample {
        total_bytes: total,
        available_bytes: available,
        full_avg10: 0.0,
        some_avg10: 0.0,
    }
}

#[test]
fn budgets_leave_host_headroom_and_never_grow_with_core_count() {
    let roomy = MutationBudget::from_sample(Some(&sample(64 * GIB, 60 * GIB))).unwrap();
    assert_eq!(roomy.memory_bytes, 8 * GIB);
    assert_eq!(roomy.reserve_bytes, 2 * GIB);
    assert!((1..=2).contains(&roomy.jobs));
    let constrained = MutationBudget::from_sample(Some(&sample(4 * GIB, GIB))).unwrap();
    assert!(constrained.memory_bytes <= (GIB - constrained.reserve_bytes) / 2);
    assert!(MutationBudget::from_sample(Some(&sample(GIB, 300 * MIB))).is_err());
}

#[test]
fn worker_limits_preserve_a_smaller_explicit_limit() {
    assert_eq!(bounded_jobs(Some("1"), 2), 1);
    for inherited in [None, Some("0"), Some("-1"), Some("64"), Some("invalid")] {
        assert_eq!(bounded_jobs(inherited, 2), 2);
    }
}

#[test]
fn unavailable_telemetry_is_described_without_claiming_containment() {
    let budget = MutationBudget::from_sample(None).unwrap();
    assert_eq!(budget.memory_bytes, 2 * GIB);
    assert!(
        budget
            .description(false)
            .contains("containment unavailable")
    );
    assert!(budget.description(true).contains("2048 MiB"));
    assert!(budget.high_bytes() <= budget.memory_bytes / 5 * 4);
    assert_eq!(budget.high_bytes() % MEMORY_ALIGNMENT, 0);
}

#[test]
fn page_rounded_kernel_controls_stay_inside_memory_ceilings() {
    for ceiling in [4 * GIB, 2_087_566_336, 2_087_566_592] {
        let maximum = align_memory_bytes(ceiling);
        let high = MutationBudget::high_memory_bytes(maximum);
        for page in [4096, 16384, 65536] {
            let actual_max = maximum.div_ceil(page) * page;
            let actual_high = high.div_ceil(page) * page;
            assert!(actual_max <= ceiling);
            assert!(actual_high <= actual_max / 5 * 4);
            assert!(actual_high > 0);
        }
    }
}
