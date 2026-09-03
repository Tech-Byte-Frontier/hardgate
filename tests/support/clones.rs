//! Clone-detector fixtures for integration tests.

use std::path::PathBuf;

/// Loop body shared by the clone-detector fixtures.
pub fn sum_loop_body() -> &'static str {
    r#"
        let mut sum = 0;
        for i in 0..100 {
            sum += i * 2;
            println!("Value: {}", sum);
        }
    "#
}

/// Two files with identical bodies for clone tests.
pub fn clone_pair(first: &str, second: &str) -> Vec<(PathBuf, String)> {
    let body = sum_loop_body();
    vec![
        (PathBuf::from(first), format!("fn foo() {{\n{body}\n}}")),
        (PathBuf::from(second), format!("fn bar() {{\n{body}\n}}")),
    ]
}

pub fn clone_config() -> hardgate::config::CloneConfig {
    hardgate::config::CloneConfig {
        enabled: true,
        min_lines: 5,
        min_tokens: 25,
        excludes: None,
    }
}
