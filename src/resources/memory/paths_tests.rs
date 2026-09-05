use super::super::fixture_tests::{invalid, invalid_with_context};
use super::{
    CgroupMount, cgroup_directories, find_cgroup2_mounts, mount_root_matches, parse_cgroup_path,
};
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

fn components(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn mount(root: &str, mount_point: &str) -> CgroupMount {
    CgroupMount {
        root: PathBuf::from(root),
        mount_point: PathBuf::from(mount_point),
    }
}

#[test]
fn cgroup_paths_skip_blank_lines_and_accept_v1_and_unified_entries() {
    assert_eq!(parse_cgroup_path("\n7:memory:/legacy\n").unwrap(), None);
    assert_eq!(
        parse_cgroup_path("\n7:memory:/legacy\n0::/tenant/job\n").unwrap(),
        Some(components(&["tenant", "job"]))
    );
}

#[test]
fn cgroup_paths_reject_empty_malformed_and_unsafe_entries() {
    for input in [
        "",
        "\n",
        "0",
        "0:",
        "0::relative",
        "0::/bad\0path",
        "0::/../outside",
        "7:memory:relative",
        "not-a-number::/job",
    ] {
        assert_eq!(
            invalid(parse_cgroup_path(input)).kind(),
            io::ErrorKind::InvalidData
        );
    }
}

#[test]
fn cgroup_paths_report_missing_path_fields() {
    let error = invalid(parse_cgroup_path("0:"));
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(
        error
            .to_string()
            .contains("malformed /proc/self/cgroup line"),
        "{error}"
    );
}

#[test]
fn cgroup_paths_reject_duplicate_unified_membership_and_controllers() {
    for input in ["0:memory:/job", "0::/one\n0::/two"] {
        let error = invalid(parse_cgroup_path(input));
        assert!(error.to_string().contains("unified cgroup"), "{error}");
    }
}

#[test]
fn mountinfo_skips_other_filesystems_and_decodes_escaped_paths() {
    let mounts = find_cgroup2_mounts(
        "1 2 3 4 5 6 - ext4 /dev rw\n42 1 0:42 / /sys\\040fs rw - cgroup2 cgroup rw\n",
    )
    .unwrap();
    assert_eq!(mounts.len(), 1);
    assert_eq!(mounts[0].root, PathBuf::from("/"));
    assert_eq!(mounts[0].mount_point, PathBuf::from("/sys fs"));
}

#[test]
fn mountinfo_rejects_missing_separator_fields_and_missing_cgroup2() {
    for input in ["1 2 3 4 5 -", "1 2 3 4 5 6 -"] {
        assert_eq!(
            invalid(find_cgroup2_mounts(input)).kind(),
            io::ErrorKind::InvalidData
        );
    }
    let error = invalid(find_cgroup2_mounts("42 1 0:42 / /sys rw - ext4 /dev rw"));
    assert!(error.to_string().contains("unified cgroup2"), "{error}");
}

#[test]
fn mountinfo_rejects_relative_traversal_nul_and_bad_escapes() {
    for root in [
        "relative",
        "/../escape",
        "/bad\\",
        "/bad\\999",
        "/bad\\000",
        "/bad\\0",
        "/bad\\00",
        "/bad\\090",
        "/bad\\009",
    ] {
        let line = format!("42 1 0:42 {root} /sys rw - cgroup2 cgroup rw");
        assert_eq!(
            invalid_with_context(find_cgroup2_mounts(&line), format!("root={root}")).kind(),
            io::ErrorKind::InvalidData,
            "root={root}"
        );
    }
}

#[test]
fn cgroup_directories_resolve_namespace_roots_and_nonmatching_paths() {
    let mount = mount("/tenant", "/sys/fs/cgroup");
    let matching = cgroup_directories(&mount, &components(&["tenant", "job"])).unwrap();
    assert_eq!(
        matching,
        vec![
            PathBuf::from("/sys/fs/cgroup/job"),
            PathBuf::from("/sys/fs/cgroup")
        ]
    );
    let nonmatching = cgroup_directories(&mount, &components(&["other", "job"])).unwrap();
    assert_eq!(
        nonmatching,
        vec![
            PathBuf::from("/sys/fs/cgroup/other/job"),
            PathBuf::from("/sys/fs/cgroup/other"),
            PathBuf::from("/sys/fs/cgroup"),
        ]
    );
}

#[test]
fn mount_root_matching_is_fail_closed_for_invalid_roots() {
    let root = mount("/tenant", "/sys/fs/cgroup");
    assert!(mount_root_matches(&root, &components(&["tenant", "job"])));
    assert!(!mount_root_matches(&root, &components(&["other", "job"])));
    assert!(mount_root_matches(
        &mount("/", "/sys/fs/cgroup"),
        &components(&["any"])
    ));
    assert!(!mount_root_matches(
        &mount("relative", "/sys/fs/cgroup"),
        &components(&["relative"])
    ));
}

#[test]
fn cgroup_directories_reject_unsafe_mount_points() {
    for mount in [mount("/", "relative"), mount("/", "/sys/../cgroup")] {
        assert_eq!(
            invalid(cgroup_directories(&mount, &components(&["job"]))).kind(),
            io::ErrorKind::InvalidData
        );
    }
}

#[test]
fn mountinfo_ignores_lines_without_separator_but_rejects_relative_mount_points() {
    let valid = "42 1 0:42 / /sys rw - cgroup2 cgroup rw";
    assert_eq!(
        find_cgroup2_mounts(&format!("unrelated line\n{valid}"))
            .unwrap()
            .len(),
        1
    );
    let invalid_mount = valid.replace("/sys", "relative");
    assert_eq!(
        invalid(find_cgroup2_mounts(&invalid_mount)).kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn resolved_membership_cannot_escape_the_mount_directory() {
    assert_eq!(
        invalid(cgroup_directories(
            &mount("/", "/sys/fs/cgroup"),
            &components(&["/elsewhere"])
        ))
        .kind(),
        io::ErrorKind::InvalidData
    );
}
