use super::super::fixture_tests::{Fixture, invalid};
use super::super::procfs::Pressure;
use super::{CgroupTelemetry, SampleState, merge_min, parse_counter, parse_limit, read_level};
use std::fs;
use std::io;

fn telemetry(total: Option<u64>, available: Option<u64>) -> CgroupTelemetry {
    CgroupTelemetry {
        total_limit: total,
        available_headroom: available,
        pressure: Pressure {
            full_avg10: 0.0,
            some_avg10: 0.0,
        },
    }
}

#[test]
fn limits_and_counters_accept_trimmed_values_and_reject_bad_values() {
    assert_eq!(
        parse_limit(" max\n".to_owned(), "memory.max").unwrap(),
        None
    );
    assert_eq!(
        parse_limit(" 42 \n".to_owned(), "memory.max").unwrap(),
        Some(42)
    );
    assert_eq!(
        parse_counter(" 42 \n".to_owned(), "memory.current").unwrap(),
        42
    );
    for value in ["", "1 2", "not-a-number", "18446744073709551616"] {
        assert_eq!(
            invalid(parse_counter(value.to_owned(), "memory.current")).kind(),
            io::ErrorKind::InvalidData
        );
    }
}

#[test]
fn read_level_handles_root_absence_partial_data_and_nonroot_files() {
    let fixture = Fixture::new("cgroup");
    let root = read_level(&fixture.root, true).unwrap();
    assert_eq!(root.maximum, None);
    assert_eq!(root.high, None);
    assert_eq!(root.current, None);

    fixture.write("memory.max", "10\n");
    assert_eq!(
        invalid(read_level(&fixture.root, true)).kind(),
        io::ErrorKind::InvalidData
    );

    fixture.write("memory.high", "5\n");
    fixture.write("memory.current", "2\n");
    fixture.write(
        "memory.pressure",
        "some avg10=1 avg60=0 avg300=0 total=1\nfull avg10=2 avg60=0 avg300=0 total=1\n",
    );
    let level = read_level(&fixture.root, false).unwrap();
    assert_eq!(level.maximum, Some(10));
    assert_eq!(level.high, Some(5));
    assert_eq!(level.current, Some(2));
    assert_eq!(level.pressure.unwrap().full_avg10, 2.0);
}

#[test]
fn read_level_reports_missing_and_invalid_nonroot_controller_files() {
    let fixture = Fixture::new("cgroup");
    fixture.write("memory.high", "5\n");
    fixture.write("memory.current", "2\n");
    assert_eq!(
        invalid(read_level(&fixture.root, false)).kind(),
        io::ErrorKind::InvalidData
    );

    fixture.write("memory.max", "10\n");
    fs::remove_file(fixture.root.join("memory.high")).unwrap();
    assert_eq!(
        invalid(read_level(&fixture.root, false)).kind(),
        io::ErrorKind::InvalidData
    );

    fixture.write("memory.high", "5\n");
    fixture.write("memory.current", "2 3\n");
    assert_eq!(
        invalid(read_level(&fixture.root, false)).kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn sample_state_tracks_missing_and_merges_every_optional_pair() {
    let mut state = SampleState::default();
    state.record(Ok(telemetry(Some(10), None))).unwrap();
    state.record(Ok(telemetry(None, Some(5)))).unwrap();
    let combined = state.finish().unwrap();
    assert_eq!(combined.total_limit, Some(10));
    assert_eq!(combined.available_headroom, Some(5));

    assert_eq!(merge_min(Some(8), Some(3)), Some(3));
    assert_eq!(merge_min(Some(8), None), Some(8));
    assert_eq!(merge_min(None, Some(3)), Some(3));
    assert_eq!(merge_min(None, None), None);
}

#[test]
fn sample_state_finishes_with_last_missing_or_invalid_data() {
    let mut missing = SampleState::default();
    missing
        .record(Err(io::Error::new(io::ErrorKind::NotFound, "missing")))
        .unwrap();
    assert_eq!(invalid(missing.finish()).kind(), io::ErrorKind::NotFound);

    let empty = SampleState::default();
    assert_eq!(invalid(empty.finish()).kind(), io::ErrorKind::InvalidData);

    let mut failed = SampleState::default();
    assert_eq!(
        invalid(failed.record(Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "denied",
        ))))
        .kind(),
        io::ErrorKind::PermissionDenied
    );
}
