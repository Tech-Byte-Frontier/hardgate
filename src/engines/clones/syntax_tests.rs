use super::regions;
use crate::engines::complexity::SupportedLanguage;
use std::path::Path;

fn retained(source: &str) -> Vec<&str> {
    let (_, tree) = SupportedLanguage::parse_file(Path::new("sample.py"), source).unwrap();
    regions(tree.root_node())
        .into_iter()
        .map(|range| &source[range])
        .collect()
}

#[test]
fn python_literal_collections_are_data_not_executable_clones() {
    for value in [
        "['a', 'b', 'c']",
        "{'a', 'b', 'c'}",
        "('a', 1, None, True, 1.5)",
        "{'name': 'a', 'rows': [('b', 1, 0.08)]}",
    ] {
        let source = format!("before()\ndata = {value}\nafter()\n");
        let parts = retained(&source);
        assert_eq!(parts, ["before()\ndata = ", "\nafter()\n"]);
    }
}

#[test]
fn python_computations_inside_collections_remain_clone_candidates() {
    for value in [
        "[compute(), 'b']",
        "{'a': compute()}",
        "(compute(), 1)",
        "{compute(), 'b'}",
        "[compute(x) for x in values]",
        "[f'{compute()}']",
        "{f'{compute()}': 1}",
        "[f'{value:{width()}}']",
    ] {
        let source = format!("data = {value}\n");
        let parts = retained(&source);
        assert!(
            parts
                .iter()
                .any(|part| part.contains("compute(") || part.contains("width()"))
        );
    }
}

#[test]
fn python_literal_boundary_does_not_join_unrelated_statements() {
    let source = "alpha()\nvalues = [1, 2, 3]\nbeta()\n";
    let parts = retained(source);
    assert_eq!(parts.len(), 2);
    assert!(
        !parts
            .iter()
            .any(|part| part.contains("alpha()") && part.contains("beta()"))
    );
}

#[test]
fn python_typed_class_fields_are_declarations() {
    let source = "class Result:\n    ticker: str\n    value: float | None = None\n    rows: list[str] = []\n    def run(self):\n        execute()\n";
    let parts = retained(source);
    assert!(!parts.iter().any(|part| part.contains("ticker: str")));
    assert!(!parts.iter().any(|part| part.contains("value: float")));
    assert!(!parts.iter().any(|part| part.contains("rows: list")));
    assert!(parts.iter().any(|part| part.contains("execute()")));
}

#[test]
fn python_executable_fields_and_local_assignments_are_retained() {
    for source in [
        "class Result:\n    value: float = compute()\n",
        "class Result:\n    value: factory() = None\n",
        "class Result:\n    value: str = f'{compute()}'\n",
        "def run():\n    value: float = 1\n",
    ] {
        assert_eq!(retained(source), [source]);
    }
}
