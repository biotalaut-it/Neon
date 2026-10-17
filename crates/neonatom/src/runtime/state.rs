use crate::syntax::{RuntimeError, RuntimeErrorKind};
use std::{cell::RefCell, rc::Rc, time::Instant};

#[derive(Debug)]
pub struct State {
    pub(crate) fuel: u64,
    pub(crate) deadline: Option<Instant>,
}
pub(crate) fn err(kind: RuntimeErrorKind, message: impl Into<String>) -> RuntimeError {
    RuntimeError {
        kind,
        message: message.into(),
    }
}
pub(crate) fn tick(state: &Rc<RefCell<State>>) -> Result<(), RuntimeError> {
    let mut s = state.borrow_mut();
    if s.deadline.is_some_and(|deadline| Instant::now() > deadline) {
        return Err(err(
            RuntimeErrorKind::Timeout,
            "evaluation time limit exceeded",
        ));
    }
    if s.fuel == 0 {
        return Err(err(
            RuntimeErrorKind::FuelExhausted,
            "evaluation fuel exhausted",
        ));
    }
    s.fuel -= 1;
    Ok(())
}
