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
fn clone_detector_ignores_python_routine_declarations() {
    let detector = CloneDetector::new(&clone_config());

    let py_a = r#"
import os
import sys
from typing import Dict, List, Optional, Tuple
from datetime import datetime, timezone

def calculate_area(radius):
    pi = 3.14159
    return pi * radius * radius
"#;
    let py_b = r#"
import os
import sys
from typing import Dict, List, Optional, Tuple
from datetime import datetime, timezone

def calculate_perimeter(length, width):
    return 2 * (length + width)
"#;
    let files = vec![
        (PathBuf::from("src/a.py"), py_a.to_string()),
        (PathBuf::from("src/b.py"), py_b.to_string()),
    ];
    let violations = detector.detect_clones(&files, Path::new(".")).unwrap();
    assert!(
        violations.is_empty(),
        "Python imports should not produce clone violations: {violations:?}"
    );
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
