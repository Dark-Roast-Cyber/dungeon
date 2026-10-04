use crate::{Error, Result};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::io::Write;

#[derive(Default)]
pub(crate) struct Output {
    pub(crate) machine: bool,
    review: RefCell<Value>,
    mutation_started: Cell<bool>,
}

impl Output {
    pub(crate) fn machine() -> Self {
        Self {
            machine: true,
            ..Self::default()
        }
    }

    pub(crate) fn message(&self, text: std::fmt::Arguments<'_>) -> Result<()> {
        if !self.machine {
            writeln!(std::io::stdout(), "{text}")?;
        }
        Ok(())
    }

    pub(crate) fn review(&self, changes: &[(String, &'static str)]) -> Result<()> {
        if self.machine {
            *self.review.borrow_mut() = json!(
                changes
                    .iter()
                    .map(|(path, change)| json!({"path":path,"change":change}))
                    .collect::<Vec<_>>()
            );
        } else {
            for (path, change) in changes {
                self.message(format_args!("{change:<9}{}", path.escape_debug()))?;
            }
        }
        Ok(())
    }

    pub(crate) fn reviewed(&self) -> Value {
        self.review.borrow().clone()
    }

    pub(crate) fn mutating(&self) {
        self.mutation_started.set(true);
    }

    pub(crate) fn error(&self, mut error: Error) -> Error {
        if self.mutation_started.get() {
            error.effects = "unknown";
        }
        error
    }
}
