#![forbid(unsafe_code)]

use ail_diagnostic::{AilResult, Diagnostic};
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Budget {
    fuel: u64,
    maximum_depth: usize,
}

impl Budget {
    #[must_use]
    pub fn new(fuel: u64, maximum_depth: usize) -> Self {
        Self {
            fuel,
            maximum_depth,
        }
    }

    pub fn consume(&mut self, amount: u64) -> AilResult<()> {
        if self.fuel < amount {
            return Err(Diagnostic::simple(
                "RUNTIME_FUEL_EXHAUSTED",
                "execution exhausted its fuel allowance",
            ));
        }
        self.fuel -= amount;
        Ok(())
    }

    pub fn check_depth(&self, depth: usize) -> AilResult<()> {
        if depth > self.maximum_depth {
            return Err(Diagnostic::new(
                "RUNTIME_DEPTH_EXHAUSTED",
                "execution exceeded its maximum call depth",
                json!({ "maxDepth": self.maximum_depth }),
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn remaining_fuel(&self) -> u64 {
        self.fuel
    }
}
