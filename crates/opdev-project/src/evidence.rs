use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use opdev_core::{Evidence, Outcome, RuleCatalog, RuleId, VerificationMethod};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Repository-relative path of the optional evidence ledger.
pub const EVIDENCE_PATH: &str = ".opdev/evidence.yaml";
const EVIDENCE_SCHEMA: &str = include_str!("../../../schema/evidence.schema.json");
const EVIDENCE_BOOTSTRAP_SCHEMA: &str =
    include_str!("../../../schema/evidence-bootstrap.schema.json");

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
    /// A new evidence file could not be created.
    #[error("could not create evidence file `{path}`: {source}")]
    Write {
        /// Evidence file path.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// YAML parsing failed.
    #[error("evidence ledger YAML is invalid: {0}")]
    Yaml(#[from] serde_saphyr::Error),
    /// YAML serialization failed.
    #[error("evidence YAML could not be serialized: {0}")]
    YamlSerialize(#[from] serde_saphyr::SerializeError),
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

/// Explicit reviewer decision for a bootstrap candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    /// No satisfying outcome has been asserted.
    ReviewRequired,
    /// The reviewer asserts that the rule passed.
    Passed,
    /// The reviewer asserts, with applicability evidence, that the rule does not apply.
    NotApplicable,
}

/// Shared evidence and per-rule reviewer decisions for one evidence scope.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceReview {
    /// Concrete evidence shared by the accepted decisions in this scope.
    pub evidence: Vec<Evidence>,
    /// One explicit decision for every generated rule candidate.
    pub decisions: BTreeMap<String, ReviewDecision>,
}

/// Fingerprint-bound review of one exact staged repository state.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeEvidenceReview {
    /// SHA-256 fingerprint of the staged Git index.
    pub fingerprint: String,
    /// Work item or other review authority for this change.
    pub work: String,
    /// Concrete evidence shared by the accepted change decisions.
    pub evidence: Vec<Evidence>,
    /// One explicit decision for every generated change-rule candidate.
    pub decisions: BTreeMap<String, ReviewDecision>,
}

/// Compact, schema-validated review input for a new evidence ledger.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceBootstrap {
    /// Bootstrap questionnaire schema version.
    pub schema: u32,
    /// Durable project-policy or capability review.
    pub project: EvidenceReview,
    /// Review bound to the exact staged change.
    pub change: ChangeEvidenceReview,
}

impl EvidenceBootstrap {
    /// Creates an unresolved questionnaire from the evaluator's current candidates.
    #[must_use]
    pub fn new(
        fingerprint: String,
        project_rules: impl IntoIterator<Item = String>,
        change_rules: impl IntoIterator<Item = String>,
    ) -> Self {
        let unresolved = |rules: Vec<String>| {
            rules
                .into_iter()
                .map(|rule| (rule, ReviewDecision::ReviewRequired))
                .collect()
        };
        Self {
            schema: 1,
            project: EvidenceReview {
                evidence: Vec::new(),
                decisions: unresolved(project_rules.into_iter().collect()),
            },
            change: ChangeEvidenceReview {
                fingerprint,
                work: String::new(),
                evidence: Vec::new(),
                decisions: unresolved(change_rules.into_iter().collect()),
            },
        }
    }

    /// Loads and schema-validates a completed bootstrap review.
    ///
    /// # Errors
    ///
    /// Returns [`EvidenceError`] when the file cannot be read or violates the
    /// bootstrap schema.
    pub fn load(path: &Path) -> Result<Self, EvidenceError> {
        let yaml = fs::read_to_string(path).map_err(|source| EvidenceError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let value: serde_json::Value = serde_saphyr::from_str(&yaml)?;
        validate_schema_document(&value, EVIDENCE_BOOTSTRAP_SCHEMA, "bootstrap")?;
        Ok(serde_saphyr::from_str(&yaml)?)
    }

    /// Serializes the review input in its canonical YAML form.
    ///
    /// # Errors
    ///
    /// Returns [`EvidenceError`] when serialization fails.
    pub fn to_yaml(&self) -> Result<String, EvidenceError> {
        Ok(serde_saphyr::to_string(self)?)
    }

    /// Serializes a questionnaire with catalog-derived comments for reviewers.
    /// Comments are informative; rule IDs and decisions remain the only parsed
    /// authority.
    ///
    /// # Errors
    ///
    /// Returns [`EvidenceError`] when serialization fails.
    pub fn to_review_yaml(&self, catalog: &RuleCatalog) -> Result<String, EvidenceError> {
        let yaml = self.to_yaml()?;
        let mut rendered = String::new();
        for line in yaml.lines() {
            if let Some((prefix, _)) = line.split_once(": review_required") {
                let rule_id = prefix.trim();
                if let Ok(rule_id) = rule_id.parse::<RuleId>()
                    && let Some(rule) = catalog.find(&rule_id)
                {
                    let indentation = &line[..line.len() - line.trim_start().len()];
                    let title = rule.title.replace(['\r', '\n'], " ");
                    let applicability = rule.applicability.replace(['\r', '\n'], " ");
                    rendered.push_str(indentation);
                    rendered.push_str("# ");
                    rendered.push_str(&title);
                    rendered.push_str("; applicability: ");
                    rendered.push_str(&applicability);
                    rendered.push('\n');
                }
            }
            rendered.push_str(line);
            rendered.push('\n');
        }
        Ok(rendered)
    }

    /// Ensures reviewed answers still describe exactly the candidates that the
    /// current evaluator and staged state produced.
    ///
    /// # Errors
    ///
    /// Returns [`EvidenceError`] for stale, added, removed, or re-scoped rules.
    pub fn validate_candidates(
        &self,
        project_rules: &[String],
        change_rules: &[String],
        fingerprint: &str,
    ) -> Result<(), EvidenceError> {
        if self.schema != 1 {
            return Err(EvidenceError::Semantic(format!(
                "unsupported bootstrap schema {}; expected 1",
                self.schema
            )));
        }
        if self.change.fingerprint != fingerprint {
            return Err(EvidenceError::Semantic(
                "the bootstrap review is stale: its change fingerprint does not match the current staged index"
                    .into(),
            ));
        }
        validate_candidate_set(
            "project",
            self.project.decisions.keys(),
            project_rules.iter(),
        )?;
        validate_candidate_set("change", self.change.decisions.keys(), change_rules.iter())?;
        Ok(())
    }

    /// Expands accepted decisions into the existing evidence-ledger contract.
    /// Unresolved decisions are omitted and never become satisfying outcomes.
    ///
    /// # Errors
    ///
    /// Returns [`EvidenceError`] when an accepted decision lacks concrete
    /// evidence, change authority, or catalog support.
    pub fn to_ledger(&self, catalog: &RuleCatalog) -> Result<EvidenceLedger, EvidenceError> {
        let project = reviewed_assertions(
            "project",
            &self.project.decisions,
            &self.project.evidence,
            catalog,
        )?;
        let change_assertions = reviewed_assertions(
            "change",
            &self.change.decisions,
            &self.change.evidence,
            catalog,
        )?;
        if project.is_empty() && change_assertions.is_empty() {
            return Err(EvidenceError::Semantic(
                "the bootstrap review has no accepted decisions; review_required is intentionally not evidence"
                    .into(),
            ));
        }
        if !change_assertions.is_empty() && self.change.work.trim().is_empty() {
            return Err(EvidenceError::Semantic(
                "accepted change decisions need a concrete work authority".into(),
            ));
        }
        let changes = if change_assertions.is_empty() {
            Vec::new()
        } else {
            vec![ChangeEvidence {
                fingerprint: self.change.fingerprint.clone(),
                work: self.change.work.trim().to_owned(),
                assertions: change_assertions,
            }]
        };
        let ledger = EvidenceLedger {
            schema: 1,
            project,
            changes,
        };
        ledger.validate(catalog)?;
        Ok(ledger)
    }
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
        validate_schema_document(&value, EVIDENCE_SCHEMA, "ledger")?;
        let ledger: Self = serde_saphyr::from_str(&yaml)?;
        ledger.validate(catalog)?;
        Ok(Some(ledger))
    }

    /// Serializes the ledger in canonical YAML form.
    ///
    /// # Errors
    ///
    /// Returns [`EvidenceError`] when serialization fails.
    pub fn to_yaml(&self) -> Result<String, EvidenceError> {
        Ok(serde_saphyr::to_string(self)?)
    }

    /// Creates `.opdev/evidence.yaml` without replacing an existing ledger.
    ///
    /// # Errors
    ///
    /// Returns [`EvidenceError`] when validation or create-new writing fails.
    pub fn write_new(&self, root: &Path, catalog: &RuleCatalog) -> Result<PathBuf, EvidenceError> {
        self.validate(catalog)?;
        let path = root.join(EVIDENCE_PATH);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|source| EvidenceError::Write {
                path: path.clone(),
                source,
            })?;
        file.write_all(self.to_yaml()?.as_bytes())
            .map_err(|source| EvidenceError::Write {
                path: path.clone(),
                source,
            })?;
        Ok(path)
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

fn validate_candidate_set<'a>(
    scope: &str,
    actual: impl Iterator<Item = &'a String>,
    expected: impl Iterator<Item = &'a String>,
) -> Result<(), EvidenceError> {
    let actual = actual.cloned().collect::<BTreeSet<_>>();
    let expected = expected.cloned().collect::<BTreeSet<_>>();
    if actual == expected {
        return Ok(());
    }
    let added = actual.difference(&expected).cloned().collect::<Vec<_>>();
    let missing = expected.difference(&actual).cloned().collect::<Vec<_>>();
    Err(EvidenceError::Semantic(format!(
        "the bootstrap {scope} candidates do not match the current evaluator (added: {}; missing: {})",
        display_rules(&added),
        display_rules(&missing)
    )))
}

fn display_rules(rules: &[String]) -> String {
    if rules.is_empty() {
        "none".into()
    } else {
        rules.join(", ")
    }
}

fn reviewed_assertions(
    scope: &str,
    decisions: &BTreeMap<String, ReviewDecision>,
    evidence: &[Evidence],
    catalog: &RuleCatalog,
) -> Result<Vec<EvidenceAssertion>, EvidenceError> {
    let accepted = decisions
        .values()
        .any(|decision| *decision != ReviewDecision::ReviewRequired);
    if accepted && evidence.is_empty() {
        return Err(EvidenceError::Semantic(format!(
            "accepted {scope} decisions need concrete shared evidence"
        )));
    }
    decisions
        .iter()
        .filter_map(|(rule_id, decision)| {
            let outcome = match decision {
                ReviewDecision::ReviewRequired => return None,
                ReviewDecision::Passed => Outcome::Passed,
                ReviewDecision::NotApplicable => Outcome::NotApplicable,
            };
            Some((rule_id, outcome))
        })
        .map(|(rule_id, outcome)| {
            let parsed = rule_id.parse::<RuleId>().map_err(|error| {
                EvidenceError::Semantic(format!("invalid bootstrap rule `{rule_id}`: {error}"))
            })?;
            let rule = catalog.find(&parsed).ok_or_else(|| {
                EvidenceError::Semantic(format!(
                    "bootstrap {scope} references unknown rule `{rule_id}`"
                ))
            })?;
            let summary = match outcome {
                Outcome::Passed => format!("Reviewed outcome: {}", rule.title),
                Outcome::NotApplicable => {
                    format!("Reviewed applicability: {}", rule.applicability)
                }
                _ => unreachable!("bootstrap decisions have satisfying outcomes only"),
            };
            Ok(EvidenceAssertion {
                rule_id: parsed,
                outcome,
                summary,
                evidence: evidence.to_vec(),
            })
        })
        .collect()
}

fn validate_schema_document(
    value: &serde_json::Value,
    schema_document: &str,
    document_name: &str,
) -> Result<(), EvidenceError> {
    let schema: serde_json::Value = serde_json::from_str(schema_document)?;
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| EvidenceError::SchemaCompile(error.to_string()))?;
    let messages = validator
        .iter_errors(value)
        .map(|error| format!("- {}: {}", error.instance_path(), error))
        .collect::<Vec<_>>();
    if messages.is_empty() {
        Ok(())
    } else {
        Err(EvidenceError::SchemaValidation(format!(
            "{document_name}:\n{}",
            messages.join("\n")
        )))
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
        validate_schema_document(&value, EVIDENCE_SCHEMA, "ledger")?;
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

    #[test]
    fn bootstrap_is_unresolved_schema_valid_and_fail_closed()
    -> Result<(), Box<dyn std::error::Error>> {
        let fingerprint = "a".repeat(64);
        let project_rules = vec!["OPDEV-SEC-001".to_owned()];
        let change_rules = vec!["OPDEV-WORK-001".to_owned()];
        let mut review = EvidenceBootstrap::new(
            fingerprint.clone(),
            project_rules.clone(),
            change_rules.clone(),
        );
        let yaml = review.to_yaml()?;
        let value: serde_json::Value = serde_saphyr::from_str(&yaml)?;
        validate_schema_document(&value, EVIDENCE_BOOTSTRAP_SCHEMA, "bootstrap")?;
        assert!(review.to_ledger(&embedded_catalog()?).is_err());

        review
            .project
            .decisions
            .insert("OPDEV-SEC-001".into(), ReviewDecision::Passed);
        assert!(review.to_ledger(&embedded_catalog()?).is_err());
        review.project.evidence.push(Evidence {
            kind: "policy".into(),
            summary: "The security policy was reviewed against the project boundary.".into(),
            location: Some("SECURITY.md".into()),
        });
        review.validate_candidates(&project_rules, &change_rules, &fingerprint)?;
        let ledger = review.to_ledger(&embedded_catalog()?)?;
        assert_eq!(ledger.project.len(), 1);
        assert!(ledger.changes.is_empty());

        let mut stale_rules = change_rules;
        stale_rules.push("OPDEV-DESIGN-001".into());
        assert!(
            review
                .validate_candidates(&project_rules, &stale_rules, &fingerprint)
                .is_err()
        );
        assert!(
            review
                .validate_candidates(&project_rules, &[], &"b".repeat(64))
                .is_err()
        );
        Ok(())
    }
}
