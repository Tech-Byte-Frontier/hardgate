use super::*;
use serde_json::json;

#[test]
fn only_explicitly_empty_native_counters_can_be_added() {
    let root = crate::fs_tests::tempdir("python-counters");
    let path = root.join("native.json");
    let mut native = json!({"meta":{"branch_coverage":true}, "files":{"constants.py":{"summary":{"num_statements":1,"covered_lines":1,"num_branches":0,"covered_branches":0},"functions":{}}}});
    let lcov = "SF:constants.py\nDA:1,1\nLF:1\nLH:1\nend_of_record\n";
    std::fs::write(&path, serde_json::to_vec(&native).unwrap()).unwrap();
    let normalized = normalize(lcov, &path).unwrap();
    assert!(normalized.contains("BRF:0\nBRH:0"));
    assert!(normalized.contains("FNF:0\nFNH:0"));
    assert!(normalized.contains("DA:1,1"));
    native["files"]["constants.py"]["summary"]["num_branches"] = json!(2);
    std::fs::write(&path, serde_json::to_vec(&native).unwrap()).unwrap();
    assert!(normalize(lcov, &path).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn partial_counter_pairs_and_missing_function_metadata_never_become_zero_evidence() {
    let root = crate::fs_tests::tempdir("python-partial-counters");
    let path = root.join("native.json");
    let mut native = json!({"meta":{"branch_coverage":true},"files":{"empty.py":{"summary":{"num_statements":0,"covered_lines":0,"num_branches":0,"covered_branches":0},"functions":{"":{"summary":{"num_statements":0,"covered_lines":0}},"empty":{"summary":{"num_statements":0,"covered_lines":0}}}}}});
    std::fs::write(&path, native.to_string()).unwrap();
    let normalized = normalize("SF:empty.py\nend_of_record\n", &path).unwrap();
    assert!(normalized.contains("LF:0\nLH:0"));
    for partial in ["LF:0", "LH:0", "FNF:0", "FNH:0", "BRF:0", "BRH:0"] {
        assert!(normalize(&format!("SF:empty.py\n{partial}\nend_of_record\n"), &path).is_err());
    }
    native["files"]["empty.py"]["functions"] =
        json!({"missed":{"summary":{"num_statements":1,"covered_lines":0}}});
    std::fs::write(&path, native.to_string()).unwrap();
    assert!(normalize("SF:empty.py\nend_of_record\n", &path).is_err());
    native["files"]["empty.py"]["functions"] = json!(null);
    std::fs::write(&path, native.to_string()).unwrap();
    assert!(normalize("SF:empty.py\nend_of_record\n", &path).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
