use super::execution_not_applicable;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn applicability(name: &str, content: &str) -> bool {
    let directory = std::env::temp_dir().join(format!(
        "hardgate-applicability-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join(name);
    std::fs::write(&path, content).unwrap();
    let result = execution_not_applicable(&path);
    std::fs::remove_dir_all(directory).unwrap();
    result
}

#[test]
fn erased_types_have_no_execution_requirement() {
    for content in [
        "export interface User { name: string }; export type Id = string;",
        "import type { User } from './user'; export type { User };",
        "declare function fetchUser(): string; declare const version: string;",
        "// no runtime statements\n",
    ] {
        assert!(applicability("types.ts", content), "{content}");
    }
    assert!(execution_not_applicable(Path::new("style.css")));
}

#[test]
fn runtime_typescript_and_unknown_source_remain_required() {
    for content in [
        "export const value = 1;",
        "export enum Direction { North, South }",
        "import './register'; export type Id = string;",
        "import { register } from './register'; register();",
        "export { value } from './value';",
        "export default () => <div/>;",
        "export type Broken = ;",
    ] {
        assert!(!applicability("source.tsx", content), "{content}");
    }
    assert!(!applicability("runtime.d.ts", "run();"));
    assert!(!execution_not_applicable(Path::new("missing.ts")));
    assert!(!execution_not_applicable(Path::new("script.py")));
}
