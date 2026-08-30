use serde::{Deserialize, Serialize};

use crate::{AggregateVerdict, Gate, Outcome, RuleId};

/// A concise, inspectable fact supporting a result.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    /// Evidence category, such as `manifest`, `command`, or `remote`.
    pub kind: String,
    /// Human-readable fact without unsupported interpretation.
    pub summary: String,
    /// Optional repository-relative path, URL, or immutable identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

/// Mechanism that produced a concrete evaluation result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationSource {
    /// Static catalog applicability could not be verified further.
    Catalog,
    /// Validated project-contract data.
    Manifest,
    /// A canonical project command.
    Command,
    /// A project-owned extension command.
    Extension,
    /// Local Git metadata.
    Git,
    /// Local CI configuration.
    Ci,
    /// Project-owned, schema-validated evidence ledger.
    Evidence,
    /// Read-only remote provider data.
    Remote,
    /// Human or agent-supplied evidence.
    Agent,
}

/// Evidence-backed evaluation of one stable core rule.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuleResult {
    /// Stable core rule ID.
    pub rule_id: RuleId,
    /// Catalog version used for the evaluation.
    pub catalog_version: u32,
    /// Exhaustive evaluation outcome.
    pub outcome: Outcome,
    /// Repository, artifact, revision, or other evaluated subject.
    pub subject: String,
    /// Mechanism responsible for the result.
    pub verifier: VerificationSource,
    /// Unix timestamp in whole seconds.
    pub evaluated_at: u64,
    /// Facts supporting the outcome.
    pub evidence: Vec<Evidence>,
    /// Limitation, failure, or remediation detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
}

/// Aggregate result for one decision gate.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GateVerdict {
    /// Evaluated gate.
    pub gate: Gate,
    /// Strict required-rule aggregate.
    pub verdict: AggregateVerdict,
    /// Blocking core rule IDs.
    pub blocking_rules: Vec<RuleId>,
    /// Blocking canonical suite or project-extension IDs.
    pub blocking_checks: Vec<String>,
}

/// JSON request written to a project extension command on standard input.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionRequest {
    /// Protocol compatibility version.
    pub protocol_version: String,
    /// Project-local extension ID.
    pub check_id: String,
    /// Absolute project root.
    pub project_root: String,
    /// Requested lifecycle stage.
    pub stage: String,
}

/// JSON response required from a successful project extension command.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionResponse {
    /// Protocol compatibility version echoed by the extension.
    pub protocol_version: String,
    /// Exhaustive outcome; an extension cannot invent a pass state.
    pub outcome: Outcome,
    /// Concise result summary.
    pub summary: String,
    /// Optional supporting evidence.
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    /// Optional limitation or remediation detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_response_rejects_unknown_outcomes_and_fields() {
        assert!(
            serde_json::from_str::<ExtensionResponse>(
                r#"{"protocol_version":"1.0.0","outcome":"maybe","summary":"no"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<ExtensionResponse>(
                r#"{"protocol_version":"1.0.0","outcome":"passed","summary":"ok","extra":true}"#
            )
            .is_err()
        );
    }

    #[test]
    fn extension_protocol_types_match_their_schemas() -> Result<(), Box<dyn std::error::Error>> {
        let request = ExtensionRequest {
            protocol_version: "1.0.0".into(),
            check_id: "license_policy".into(),
            project_root: "/project".into(),
            stage: "pre_merge".into(),
        };
        let response = ExtensionResponse {
            protocol_version: "1.0.0".into(),
            outcome: Outcome::Passed,
            summary: "policy satisfied".into(),
            evidence: vec![Evidence {
                kind: "file".into(),
                summary: "license allow-list found".into(),
                location: Some("policy/licenses.toml".into()),
            }],
            diagnostic: None,
        };
        for (value, source) in [
            (
                serde_json::to_value(request)?,
                include_str!("../../../schema/extension-request.schema.json"),
            ),
            (
                serde_json::to_value(response)?,
                include_str!("../../../schema/extension-response.schema.json"),
            ),
        ] {
            let schema: serde_json::Value = serde_json::from_str(source)?;
            let validator = jsonschema::validator_for(&schema)?;
            let errors: Vec<_> = validator.iter_errors(&value).collect();
            assert!(errors.is_empty(), "schema errors: {errors:#?}");
        }
        Ok(())
    }
}
