use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use opdev_core::{Evidence, Outcome, RuleCatalog, RuleId, VerificationMethod};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Repository-relative path of the optional evidence ledger.
pub const EVIDENCE_PATH: &str = ".opdev/evidence.yaml";
const EVIDENCE_SCHEMA: &str = include_str!("../../../schema/evidence.schema.json");

/// Failure while loading, validating, or binding project evidence.
#[derive(Debug, Error)]
pub enum EvidenceError {
    /// The evidence ledger could not be read.
    #[error("could not read evidence ledger `{path}`: {source}")]
    Read {
        /// Ledger path.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// YAML parsing failed.
    #[error("evidence ledger YAML is invalid: {0}")]
    Yaml(#[from] serde_saphyr::Error),
    /// Bundled schema JSON is malformed.
    #[error("the bundled evidence schema is invalid: {0}")]
    SchemaDocument(#[from] serde_json::Error),
    /// Bundled schema compilation failed.
    #[error("the bundled evidence schema could not be compiled: {0}")]
    SchemaCompile(String),
    /// The ledger violated its JSON Schema.
    #[error("evidence ledger schema validation failed:\n{0}")]
    SchemaValidation(String),
    /// A ledger assertion is inconsistent with the rule catalog.
    #[error("evidence ledger is inconsistent: {0}")]
    Semantic(String),
    /// Git could not produce an index fingerprint.
    #[error("could not fingerprint the staged Git index: {0}")]
    Git(String),
}

/// One evidence-backed assertion for a stable core rule.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceAssertion {
    /// Stable core rule ID.
    pub rule_id: RuleId,
    /// Satisfying outcome asserted by the evidence owner.
    pub outcome: Outcome,
    /// Concise interpretation of the evidence.
    pub summary: String,
    /// Concrete, reviewable evidence references.
    pub evidence: Vec<Evidence>,
}

/// Assertions bound to one exact staged repository state.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeEvidence {
    /// SHA-256 fingerprint produced by [`staged_fingerprint`].
    pub fingerprint: String,
    /// Work item, change request, or other review authority.
    pub work: String,
    /// Assertions that apply only to this fingerprint.
    pub assertions: Vec<EvidenceAssertion>,
}

/// Optional evidence supplied by an initialized project.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceLedger {
    /// Ledger schema version.
    pub schema: u32,
    /// Assertions about durable project policy or capability.
    #[serde(default)]
    pub project: Vec<EvidenceAssertion>,
    /// Assertions about exact staged change states.
    #[serde(default)]
    pub changes: Vec<ChangeEvidence>,
}

impl EvidenceLedger {
    /// Loads an optional evidence ledger. A missing file produces `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns [`EvidenceError`] for read, parse, schema, or semantic failures.
    pub fn load_optional(
        root: &Path,
        catalog: &RuleCatalog,
    ) -> Result<Option<Self>, EvidenceError> {
        let path = root.join(EVIDENCE_PATH);
        let yaml = match fs::read_to_string(&path) {
            Ok(yaml) => yaml,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(EvidenceError::Read { path, source }),
        };
        let value: serde_json::Value = serde_saphyr::from_str(&yaml)?;
        validate_schema(&value)?;
        let ledger: Self = serde_saphyr::from_str(&yaml)?;
        ledger.validate(catalog)?;
        Ok(Some(ledger))
    }

    fn validate(&self, catalog: &RuleCatalog) -> Result<(), EvidenceError> {
        if self.schema != 1 {
            return Err(EvidenceError::Semantic(format!(
                "unsupported schema {}; expected 1",
                self.schema
            )));
        }
        validate_assertions("project", &self.project, catalog)?;
        let mut fingerprints = HashSet::new();
        for change in &self.changes {
            if !fingerprints.insert(change.fingerprint.as_str()) {
                return Err(EvidenceError::Semantic(format!(
                    "duplicate change fingerprint `{}`",
                    change.fingerprint
                )));
            }
            validate_assertions(
                &format!("change `{}`", change.fingerprint),
                &change.assertions,
                catalog,
            )?;
        }
        Ok(())
    }

    /// Returns the assertions matching an exact staged fingerprint.
    #[must_use]
    pub fn matching_change(&self, fingerprint: &str) -> Option<&ChangeEvidence> {
        self.changes
            .iter()
            .find(|change| change.fingerprint == fingerprint)
    }
}

/// Computes a deterministic SHA-256 fingerprint of staged Git entries while
/// excluding the ledger itself.
///
/// Blob identities, modes, and paths are included. The working tree must not
/// contain unstaged tracked changes or untracked files other than the ledger,
/// otherwise change evidence could omit material content and the function
/// fails closed.
///
/// # Errors
///
/// Returns [`EvidenceError::Git`] when Git fails or the working tree contains
/// content not represented by the staged index.
pub fn staged_fingerprint(root: &Path) -> Result<String, EvidenceError> {
    reject_unindexed_content(root)?;
    let output = git_output(root, &["ls-files", "--stage", "-z"])?;
    let mut entries = output
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .filter(|entry| {
            entry
                .splitn(2, |byte| *byte == b'\t')
                .nth(1)
                .is_none_or(|path| path != EVIDENCE_PATH.as_bytes())
        })
        .collect::<Vec<_>>();
    entries.sort_unstable();
    let mut hasher = Sha256::new();
    for entry in entries {
        hasher.update(entry);
        hasher.update([0]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn reject_unindexed_content(root: &Path) -> Result<(), EvidenceError> {
    let unstaged = git_output(root, &["diff", "--name-only", "-z"])?;
    let untracked = git_output(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    let omitted = unstaged
        .split(|byte| *byte == 0)
        .chain(untracked.split(|byte| *byte == 0))
        .filter(|path| !path.is_empty() && *path != EVIDENCE_PATH.as_bytes())
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .collect::<Vec<_>>();
    if omitted.is_empty() {
        Ok(())
    } else {
        Err(EvidenceError::Git(format!(
            "stage all material changes before binding evidence; unindexed: {}",
            omitted.join(", ")
        )))
    }
}

fn git_output(root: &Path, arguments: &[&str]) -> Result<Vec<u8>, EvidenceError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .map_err(|error| EvidenceError::Git(error.to_string()))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(EvidenceError::Git(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ))
    }
}

fn validate_assertions(
    owner: &str,
    assertions: &[EvidenceAssertion],
    catalog: &RuleCatalog,
) -> Result<(), EvidenceError> {
    let mut rules = HashSet::new();
    for assertion in assertions {
        if !rules.insert(&assertion.rule_id) {
            return Err(EvidenceError::Semantic(format!(
                "{owner} repeats rule `{}`",
                assertion.rule_id
            )));
        }
        if !matches!(assertion.outcome, Outcome::Passed | Outcome::NotApplicable) {
            return Err(EvidenceError::Semantic(format!(
                "{owner} rule `{}` may assert only passed or not_applicable",
                assertion.rule_id
            )));
        }
        if assertion.summary.trim().is_empty() || assertion.evidence.is_empty() {
            return Err(EvidenceError::Semantic(format!(
                "{owner} rule `{}` needs a summary and concrete evidence",
                assertion.rule_id
            )));
        }
        let rule = catalog.find(&assertion.rule_id).ok_or_else(|| {
            EvidenceError::Semantic(format!(
                "{owner} references unknown rule `{}`",
                assertion.rule_id
            ))
        })?;
        if !rule.verification.contains(&VerificationMethod::Evidence)
            && !rule.verification.contains(&VerificationMethod::Agent)
        {
            return Err(EvidenceError::Semantic(format!(
                "{owner} cannot satisfy rule `{}` through reviewed project evidence",
                assertion.rule_id
            )));
        }
    }
    Ok(())
}

fn validate_schema(value: &serde_json::Value) -> Result<(), EvidenceError> {
    let schema: serde_json::Value = serde_json::from_str(EVIDENCE_SCHEMA)?;
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| EvidenceError::SchemaCompile(error.to_string()))?;
    let messages = validator
        .iter_errors(value)
        .map(|error| format!("- {}: {}", error.instance_path(), error))
        .collect::<Vec<_>>();
    if messages.is_empty() {
        Ok(())
    } else {
        Err(EvidenceError::SchemaValidation(messages.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opdev_core::embedded_catalog;

    #[test]
    fn ledger_matches_schema_and_rejects_non_satisfying_claims()
    -> Result<(), Box<dyn std::error::Error>> {
        let valid = r"
schema: 1
project:
  - rule_id: OPDEV-SEC-001
    outcome: passed
    summary: Security policy is reviewed.
    evidence:
      - kind: policy
        summary: Vulnerability reporting and trust boundaries are documented.
        location: SECURITY.md
changes: []
";
        let value: serde_json::Value = serde_saphyr::from_str(valid)?;
        validate_schema(&value)?;
        let ledger: EvidenceLedger = serde_saphyr::from_str(valid)?;
        ledger.validate(&embedded_catalog()?)?;

        let invalid = valid.replace("outcome: passed", "outcome: unverified");
        let ledger: EvidenceLedger = serde_saphyr::from_str(&invalid)?;
        assert!(ledger.validate(&embedded_catalog()?).is_err());
        Ok(())
    }

    #[test]
    fn fingerprint_excludes_only_the_ledger() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let root = directory.path();
        assert!(
            Command::new("git")
                .arg("init")
                .arg(root)
                .status()?
                .success()
        );
        fs::write(root.join("source.txt"), "one")?;
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(root)
                .args(["add", "source.txt"])
                .status()?
                .success()
        );
        let first = staged_fingerprint(root)?;
        fs::create_dir(root.join(".opdev"))?;
        fs::write(root.join(EVIDENCE_PATH), "schema: 1")?;
        let second = staged_fingerprint(root)?;
        assert_eq!(first, second);
        fs::write(root.join("untracked.txt"), "material")?;
        assert!(staged_fingerprint(root).is_err());
        Ok(())
    }
}
