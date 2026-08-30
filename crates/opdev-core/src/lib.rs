//! Normative domain types and the embedded `OpDev` rule catalog.

#![forbid(unsafe_code)]

mod assurance;
mod catalog;
mod evidence;
mod outcome;

pub use assurance::{
    AssuranceProfile, ProfileError, ProfileRequirement, ProfileSource, ProfileStatus,
    RequirementCoverage, embedded_profiles, resolve_profile,
};
pub use catalog::{
    CatalogError, Gate, Rule, RuleCatalog, RuleId, RuleKind, Source, VerificationMethod,
    embedded_catalog,
};
pub use evidence::{
    Evidence, ExtensionRequest, ExtensionResponse, GateVerdict, RuleResult, VerificationSource,
};
pub use outcome::{AggregateVerdict, Outcome};

/// Project-manifest schema understood by this release.
pub const PROJECT_SCHEMA_VERSION: u32 = 1;

/// Project-command extension protocol understood by this release.
pub const EXTENSION_PROTOCOL_VERSION: &str = "1.0.0";
