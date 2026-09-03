//! Dead-code fixtures for integration tests.

pub fn dead_code_analyzer(entry_points: Vec<String>) -> hardgate::engines::DeadCodeAnalyzer {
    let config = hardgate::config::DeadCodeConfig {
        enabled: true,
        entry_points,
        exclude: vec![],
    };
    hardgate::engines::DeadCodeAnalyzer::new(&config)
}
