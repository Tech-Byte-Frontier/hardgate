use super::super::fixture_tests::{Fixture, invalid};
use super::{
    directory_exists, parse_meminfo, parse_pressure, read_optional, read_pressure, read_required,
};
use std::fs;
use std::io;

#[test]
fn read_helpers_handle_success_missing_invalid_data_and_non_files() {
    let fixture = Fixture::new("procfs");
    let value = fixture.root.join("value");
    fs::write(&value, "ok\n").unwrap();
    assert_eq!(read_required(&value).unwrap(), "ok\n");
    assert_eq!(read_optional(&value).unwrap().as_deref(), Some("ok\n"));

    let missing = fixture.root.join("missing");
    assert_eq!(read_optional(&missing).unwrap(), None);
    assert_eq!(
        invalid(read_required(&missing)).kind(),
        io::ErrorKind::NotFound
    );

    let invalid_utf8 = fixture.root.join("invalid-utf8");
    fs::write(&invalid_utf8, [0xff_u8]).unwrap();
    assert_eq!(
        invalid(read_required(&invalid_utf8)).kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        invalid(read_optional(&invalid_utf8)).kind(),
        io::ErrorKind::InvalidData
    );

    let directory = fixture.root.join("directory");
    fs::create_dir(&directory).unwrap();
    assert!(directory_exists(&directory).unwrap());
    assert!(!directory_exists(&value).unwrap());
    assert!(!directory_exists(&missing).unwrap());
    let child_of_file = value.join("child");
    assert!(
        invalid(directory_exists(&child_of_file))
            .to_string()
            .contains("failed to inspect")
    );
    assert!(
        invalid(read_optional(&child_of_file))
            .to_string()
            .contains("failed to read")
    );
}

#[test]
fn meminfo_rejects_duplicates_missing_values_invalid_units_and_overflow() {
    let valid = "comment\nOther: ignored\nMemTotal: 2 kB\nMemAvailable: 1 kB\n";
    assert_eq!(parse_meminfo(valid).unwrap(), (2048, 1024));
    for input in [
        "MemTotal: 2 kB\nMemTotal: 3 kB\nMemAvailable: 1 kB\n",
        "MemTotal:\nMemAvailable: 1 kB\n",
        "MemTotal: nope kB\nMemAvailable: 1 kB\n",
        "MemTotal: 2 MB\nMemAvailable: 1 kB\n",
        "MemTotal: 2 kB extra\nMemAvailable: 1 kB\n",
        "MemAvailable: 1 kB\n",
    ] {
        assert_eq!(
            invalid(parse_meminfo(input)).kind(),
            io::ErrorKind::InvalidData
        );
    }
}

#[test]
fn pressure_parser_handles_blank_fields_and_rejects_duplicates() {
    let pressure =
        parse_pressure("\nsome avg60=0 avg10=2.5 total=1\nfull avg60=0 avg10=0.5 total=1\n")
            .unwrap();
    assert_eq!(pressure.some_avg10, 2.5);
    assert_eq!(pressure.full_avg10, 0.5);

    for input in [
        "other avg10=1\nfull avg10=0\n",
        "some avg60=0\nfull avg10=0\n",
        "some avg10=1 avg10=2\nfull avg10=0\n",
        "some avg10=1\nfull avg10=0\nfull avg10=1\n",
        "some avg10=1\n",
    ] {
        assert_eq!(
            invalid(parse_pressure(input)).kind(),
            io::ErrorKind::InvalidData
        );
    }
}

#[test]
fn pressure_parser_reports_non_numeric_avg10() {
    let error = invalid(parse_pressure("some avg10=not-a-number\nfull avg10=0\n"));
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(
        error.to_string().contains("invalid some PSI avg10"),
        "{error}"
    );
}

#[test]
fn read_pressure_returns_default_for_absence_and_parses_private_files() {
    let fixture = Fixture::new("procfs");
    let missing = fixture.root.join("missing-pressure");
    let default = read_pressure(&missing).unwrap();
    assert_eq!(default.full_avg10, 0.0);
    assert_eq!(default.some_avg10, 0.0);

    let path = fixture.root.join("pressure");
    fs::write(
        &path,
        "some avg10=1 avg60=0 avg300=0 total=1\nfull avg10=2 avg60=0 avg300=0 total=1\n",
    )
    .unwrap();
    let pressure = read_pressure(&path).unwrap();
    assert_eq!(pressure.full_avg10, 2.0);
    assert_eq!(pressure.some_avg10, 1.0);
}
