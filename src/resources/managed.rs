//! Optional aggregate Linux containment. Other platforms retain admission,
//! serialization, worker limits and the existing process-group cleanup.
#[cfg(target_os = "linux")]
#[path = "managed/linux.rs"]
mod platform;

#[cfg(target_os = "linux")]
pub(crate) use platform::ManagedCommand;

#[cfg(not(target_os = "linux"))]
pub(crate) struct ManagedCommand;

#[cfg(not(target_os = "linux"))]
impl ManagedCommand {
    pub(crate) fn prepare(
        _command: &mut std::process::Command,
        _budget: super::MutationBudget,
        _timeout: std::time::Duration,
    ) -> std::io::Result<Option<Self>> {
        Ok(None)
    }

    pub(crate) fn poll(
        &mut self,
        _exited: Option<std::process::ExitStatus>,
    ) -> std::io::Result<Option<std::process::ExitStatus>> {
        Ok(None)
    }

    pub(crate) fn stop(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    pub(crate) fn timed_out(
        &self,
        launched: std::time::Instant,
        timeout: std::time::Duration,
    ) -> bool {
        launched.elapsed() >= timeout
    }
}
