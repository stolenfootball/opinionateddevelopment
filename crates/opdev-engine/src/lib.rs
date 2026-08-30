//! Evidence collection, safe command execution, and strict gate aggregation.

#![forbid(unsafe_code)]

mod command;
mod evaluator;
mod report;

pub use command::{CommandError, Execution, execute};
pub use evaluator::{CheckOptions, EvaluationError, evaluate, reaggregate};
pub use report::{CheckKind, CheckReport, CheckResult};
