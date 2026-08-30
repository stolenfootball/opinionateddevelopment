use opdev_core::{Evidence, Gate, GateVerdict, Outcome, RuleResult};
use serde::{Deserialize, Serialize};

/// Origin of an executable check result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    /// Canonical test or quality suite.
    Suite,
    /// Project-owned extension protocol command.
    Extension,
}

/// Result of an executable suite or project extension.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CheckResult {
    /// Project-local check identifier.
    pub id: String,
    /// Check origin.
    pub kind: CheckKind,
    /// Whether this result contributes to gate aggregation.
    pub blocking: bool,
    /// Gates affected by a blocking result.
    pub gates: Vec<Gate>,
    /// Exhaustive result outcome.
    pub outcome: Outcome,
    /// Concise evidence summary.
    pub summary: String,
    /// Structured evidence returned by the check.
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    /// Captured bounded standard output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    /// Captured bounded standard error or diagnostic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    /// Execution duration in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u128>,
}

/// Complete machine-readable result of `opdev check`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CheckReport {
    /// Report schema version.
    pub schema: u32,
    /// Rule catalog version.
    pub catalog_version: u32,
    /// Evaluated project root.
    pub subject: String,
    /// Unix timestamp in whole seconds.
    pub evaluated_at: u64,
    /// Core rule results in catalog order.
    pub rules: Vec<RuleResult>,
    /// Executed canonical and extension checks.
    pub checks: Vec<CheckResult>,
    /// Strict aggregates for every decision gate.
    pub gates: Vec<GateVerdict>,
}

impl CheckReport {
    /// Returns whether a named gate strictly passed.
    #[must_use]
    pub fn gate_passed(&self, gate: Gate) -> bool {
        self.gates
            .iter()
            .find(|result| result.gate == gate)
            .is_some_and(|result| result.verdict == opdev_core::AggregateVerdict::Passed)
    }
}
