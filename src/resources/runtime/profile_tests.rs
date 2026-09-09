use super::*;

#[test]
fn automatic_jobs_leave_cpu_headroom_and_cap_large_hosts() {
    for (cpus, expected) in [(1, 1), (2, 1), (4, 2), (8, 4), (16, 8), (32, 8), (128, 8)] {
        assert_eq!(select(None, None, cpus).unwrap(), expected);
    }
}

#[test]
fn inherited_jobs_do_not_shrink_when_reexecuted_inside_the_quota() {
    assert_eq!(select(None, Some("8"), 8).unwrap(), 8);
    assert_eq!(select(Some(4), Some("8"), 32).unwrap(), 4);
    assert_eq!(select(Some(16), None, 32).unwrap(), 16);
    for invalid in ["0", "65", "-1", "", "many", "999999999999999999999"] {
        assert!(select(None, Some(invalid), 32).is_err(), "{invalid}");
    }
    assert!(select(Some(0), None, 32).is_err());
    assert!(select(Some(65), None, 32).is_err());
}

#[test]
fn resource_ceilings_scale_with_jobs_and_remain_finite() {
    assert_eq!(task_limit(1), 256);
    assert_eq!(task_limit(2), 256);
    assert_eq!(task_limit(8), 1024);
    assert_eq!(task_limit(64), 8192);
    let gib = 1024 * 1024 * 1024;
    assert_eq!(memory_ceiling(2), 4 * gib);
    assert_eq!(memory_ceiling(8), 8 * gib);
    assert_eq!(memory_ceiling(64), 16 * gib);
}
