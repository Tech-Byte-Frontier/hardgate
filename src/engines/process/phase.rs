//! Thread-local phase names keep parallel library callers independent.
use std::cell::RefCell;
thread_local! { static CURRENT: RefCell<Option<String>> = const { RefCell::new(None) }; }
pub(crate) struct Phase(Option<String>);
pub(crate) fn set(name: &str) -> Phase {
    Phase(CURRENT.with(|value| value.replace(Some(name.into()))))
}
pub(super) fn current() -> Option<String> {
    CURRENT.with(|value| value.borrow().clone())
}
impl Drop for Phase {
    fn drop(&mut self) {
        CURRENT.with(|value| value.replace(self.0.take()));
    }
}
