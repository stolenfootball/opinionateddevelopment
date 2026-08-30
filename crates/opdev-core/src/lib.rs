//! Normative domain types and the embedded `OpDev` rule catalog.

#![forbid(unsafe_code)]

mod catalog;
mod outcome;

pub use catalog::{
    CatalogError, Gate, Rule, RuleCatalog, RuleId, RuleKind, Source, VerificationMethod,
    embedded_catalog,
};
pub use outcome::{AggregateVerdict, Outcome};

/// Project-manifest schema understood by this release.
pub const PROJECT_SCHEMA_VERSION: u32 = 1;

/// Project-command extension protocol understood by this release.
pub const EXTENSION_PROTOCOL_VERSION: &str = "1.0.0";
