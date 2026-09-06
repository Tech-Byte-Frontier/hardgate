use hardgate::config::CloneConfig;
use hardgate::engines::CloneDetector;
use std::path::{Path, PathBuf};

fn clone_config() -> CloneConfig {
    CloneConfig {
        enabled: true,
        min_lines: 5,
        min_tokens: 25,
        excludes: None,
    }
}

#[test]
fn semicolon_free_type_alias_does_not_hide_following_executable_clones() {
    let detector = CloneDetector::new(&clone_config());
    let body = "\nfunction calculate(value: number) {\n const first = value + 1\n const second = first * 2\n const third = second - 3\n const fourth = third / 4\n return fourth + value\n}\n";
    for declaration in ["type Amount = number", "type Amount = {\n value: number\n}"] {
        let files = vec![
            (
                PathBuf::from("src/first.ts"),
                format!("{declaration}{body}"),
            ),
            (
                PathBuf::from("src/second.ts"),
                format!("{declaration}{body}"),
            ),
        ];
        assert!(
            !detector
                .detect_clones(&files, Path::new("."))
                .unwrap()
                .is_empty(),
            "declaration hid executable duplication: {declaration}"
        );
    }
}

#[test]
fn clone_detector_ignores_typescript_routine_declarations() {
    let detector = CloneDetector::new(&clone_config());
    let ts_a = r#"
import { useState, useEffect, useCallback } from 'react';
import type { FC, ReactNode } from 'react';
export type UserId = string;
export type UserProps = {
    id: UserId;
    name: string;
};

export function render_a(name: string) {
    const greeting = "hello " + name;
    return greeting.toUpperCase();
}
"#;
    let ts_b = r#"
import { useState, useEffect, useCallback } from 'react';
import type { FC, ReactNode } from 'react';
export type UserId = string;
export type UserProps = {
    id: UserId;
    name: string;
};

export function render_b(items: number[]) {
    let sum = 0;
    for (const item of items) {
        sum += item;
    }
    return sum;
}
"#;
    let files = vec![
        (PathBuf::from("src/a.tsx"), ts_a.to_string()),
        (PathBuf::from("src/b.tsx"), ts_b.to_string()),
    ];
    let violations = detector.detect_clones(&files, Path::new(".")).unwrap();
    assert!(
        violations.is_empty(),
        "TypeScript imports and type aliases should not produce clone violations: {violations:?}"
    );
}

#[test]
fn schema_literal_repetitions_complete_without_capacity_error() {
    let detector = CloneDetector::new(&clone_config());
    let schema_content = (0..200)
        .map(|i| format!("field_{i}: string;\n"))
        .collect::<String>();
    let file_a = format!("export interface SchemaA {{\n{schema_content}}}\n");
    let file_b = format!("export interface SchemaB {{\n{schema_content}}}\n");
    let files = vec![
        (PathBuf::from("src/schema_a.ts"), file_a),
        (PathBuf::from("src/schema_b.ts"), file_b),
    ];
    let result = detector.detect_clones_checked(&files, Path::new("."));
    assert!(
        result.is_ok(),
        "200-repetition schema must not exceed capacity under limit 512: {:?}",
        result.err()
    );
}
