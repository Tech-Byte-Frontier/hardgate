use super::super::fixture_tests::Fixture;
use super::*;

#[test]
fn clean_cache_is_reclaimable_but_tmpfs_dirty_and_writeback_are_not() {
    let fixture = Fixture::new("reclaim");
    assert_eq!(working_bytes(&fixture.root, 1000).unwrap(), 1000);
    fixture.write(
        "memory.stat",
        "file 800\nshmem 100\nfile_dirty 200\nfile_writeback 50\n",
    );
    assert_eq!(working_bytes(&fixture.root, 1000).unwrap(), 550);
    fixture.write(
        "memory.stat",
        "file 100\nshmem 200\nfile_dirty 0\nfile_writeback 0\n",
    );
    assert_eq!(working_bytes(&fixture.root, 1000).unwrap(), 1000);
}

#[test]
fn malformed_cache_telemetry_cannot_increase_available_memory() {
    let fixture = Fixture::new("reclaim-invalid");
    for stat in [
        "file 100\n",
        "file bad\n",
        "file 100 extra\n",
        "file 100\nfile 100\n",
    ] {
        fixture.write("memory.stat", stat);
        assert!(working_bytes(&fixture.root, 1000).is_err());
    }
}
