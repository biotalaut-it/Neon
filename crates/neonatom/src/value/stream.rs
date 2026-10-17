use super::Value;
use crate::{
    runtime::{err, tick, State},
    syntax::{RuntimeError, RuntimeErrorKind},
};
use std::{cell::RefCell, fmt, rc::Rc};

type LazyIterator = Box<dyn Iterator<Item = Result<Value, RuntimeError>>>;
type SharedIterator = Rc<RefCell<LazyIterator>>;

#[derive(Clone)]
pub struct Stream {
    inner: SharedIterator,
    pub(crate) state: Rc<RefCell<State>>,
    output_limit: usize,
}
impl Stream {
    pub(crate) fn new<I: Iterator<Item = Result<Value, RuntimeError>> + 'static>(
        iter: I,
        state: Rc<RefCell<State>>,
        output_limit: usize,
    ) -> Self {
        Self {
            inner: Rc::new(RefCell::new(Box::new(iter))),
            state,
            output_limit,
        }
    }
    fn next_item(&mut self) -> Option<Result<Value, RuntimeError>> {
        if let Err(e) = tick(&self.state) {
            return Some(Err(e));
        }
        self.inner.borrow_mut().next()
    }
    pub fn take(self, mut count: u64) -> Self {
        let state = self.state.clone();
        let limit = self.output_limit;
        let mut iter = self;
        Self::new(
            std::iter::from_fn(move || {
                if count == 0 {
                    return None;
                }
                let next = iter.next_item();
                if next.is_some() {
                    count -= 1;
                }
                next
            }),
            state,
            limit,
        )
    }
    pub fn collect_limited(mut self, count: usize) -> Result<Vec<Value>, RuntimeError> {
        let mut out = Vec::new();
        let mut bytes = 0usize;
        while out.len() < count {
            match self.next() {
                None => break,
                Some(Err(e)) => return Err(e),
                Some(Ok(v)) => {
                    bytes = bytes.saturating_add(v.to_string().len());
                    if bytes > self.output_limit {
                        return Err(err(
                            RuntimeErrorKind::OutputLimit,
                            "output byte limit exceeded",
                        ));
                    }
                    out.push(v);
                }
            }
        }
        Ok(out)
    }
}
impl Iterator for Stream {
    type Item = Result<Value, RuntimeError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_item()
    }
}
impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<lazy-stream>")
    }
}
