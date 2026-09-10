//! Thread-local phase names keep parallel library callers independent.
use std::cell::RefCell;
thread_local! { static CURRENT: RefCell<Option<String>> = const { RefCell::new(None) }; }
thread_local! { static COMMAND: RefCell<Option<Vec<String>>> = const { RefCell::new(None) }; }
pub(super) struct CommandName(Option<Vec<String>>);
pub(super) fn command(tokens: &[String]) -> CommandName {
    CommandName(COMMAND.with(|value| value.replace(Some(tokens.to_vec()))))
}
pub(super) fn current_command() -> Option<Vec<String>> {
    COMMAND.with(|value| value.borrow().clone())
}
impl Drop for CommandName {
    fn drop(&mut self) {
        COMMAND.with(|value| value.replace(self.0.take()));
    }
}
pub(crate) struct Phase(Option<String>);
pub(crate) fn set(name: &str) -> Phase {
    Phase(CURRENT.with(|value| value.replace(Some(name.into()))))
}
pub(crate) fn nested(name: &str) -> Phase {
    set(&format!(
        "{}:{name}",
        current().unwrap_or_else(|| "evidence".into())
    ))
}
pub(super) fn current() -> Option<String> {
    CURRENT.with(|value| value.borrow().clone())
}
impl Drop for Phase {
    fn drop(&mut self) {
        CURRENT.with(|value| value.replace(self.0.take()));
    }
}
