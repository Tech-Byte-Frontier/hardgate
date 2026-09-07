#![cfg(target_os = "linux")]

#[path = "support/evidence_tests.rs"]
mod protocol_tests;

use protocol_tests::{Project, SOURCE, assert_exit, lcov, mutation};
use serde_json::{Value, json};

#[path = "evidence/boundary_cases_tests.rs"]
mod boundary_cases_tests;
#[path = "evidence/diagnostic_cases_tests.rs"]
mod diagnostic_cases_tests;
#[path = "evidence/lifecycle_cases_tests.rs"]
mod lifecycle_cases_tests;
#[path = "evidence/producer_cases_tests.rs"]
mod producer_cases_tests;
