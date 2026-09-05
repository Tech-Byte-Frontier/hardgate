//! Resource admission and containment for native mutation workloads.
mod budget;
pub(crate) mod input;
mod lease;
pub(crate) mod managed;
mod memory;

use std::cell::RefCell;
use std::io;
use std::time::{Duration, Instant};

pub(crate) use budget::MutationBudget;

pub(crate) const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_SNAPSHOT_BYTES: usize = MAX_SOURCE_BYTES + 64 * 1024;

const SAMPLE_INTERVAL: Duration = Duration::from_millis(250);

thread_local! {
    static LAST_SAMPLE: RefCell<Option<Instant>> = const { RefCell::new(None) };
}

pub(crate) struct MutationGuard {
    _lease: lease::MutationLease,
    pub(crate) budget: MutationBudget,
}

impl MutationGuard {
    pub(crate) fn acquire() -> io::Result<Self> {
        crate::cancellation::install()?;
        let lease = lease::MutationLease::acquire()?;
        let sample = memory::sample()?;
        let budget = MutationBudget::from_sample(sample.as_ref())?;
        if let Some(sample) = sample {
            sample.check(budget.reserve_bytes)?;
        }
        Ok(Self {
            _lease: lease,
            budget,
        })
    }
}

/// Snapshot loops and child polling share a throttled pressure check. Admission
/// always takes a fresh sample, so this cache cannot admit a new workload.
pub(crate) fn check_pressure() -> io::Result<()> {
    crate::cancellation::check()?;
    let due = LAST_SAMPLE.with(|last| {
        last.borrow()
            .is_none_or(|at| at.elapsed() >= SAMPLE_INTERVAL)
    });
    if !due {
        return Ok(());
    }
    if let Some(sample) = memory::sample()? {
        sample.check(budget::reserve_bytes(sample.total_bytes))?;
    }
    LAST_SAMPLE.with(|last| *last.borrow_mut() = Some(Instant::now()));
    Ok(())
}
