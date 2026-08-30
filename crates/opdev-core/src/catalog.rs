use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

const EMBEDDED_CATALOG: &str = include_str!("../../../rules/core.yaml");

/// Failures encountered while loading the normative rule catalog.
#[derive(Debug, Error)]
pub enum CatalogError {
    /// The YAML document could not be parsed.
    #[error("the OpDev rule catalog is invalid YAML: {0}")]
    Yaml(#[from] serde_saphyr::Error),

    /// A rule ID is malformed.
    #[error("invalid rule ID `{0}`")]
    InvalidRuleId(String),

    /// Two catalog entries use the same stable ID.
    #[error("duplicate rule ID `{0}`")]
    DuplicateRuleId(RuleId),

    /// The catalog does not contain any rules.
    #[error("the OpDev rule catalog contains no rules")]
    Empty,
}

/// A stable `OpDev` rule identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct RuleId(String);

impl RuleId {
    /// Returns the identifier as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for RuleId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for RuleId {
    type Err = CatalogError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parts: Vec<_> = value.split('-').collect();
        let valid = parts.len() >= 3
            && parts.iter().all(|part| {
                !part.is_empty()
                    && part
                        .chars()
                        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
            })
            && parts.last().is_some_and(|suffix| {
                suffix.len() == 3 && suffix.chars().all(|ch| ch.is_ascii_digit())
            });

        if valid {
            Ok(Self(value.to_owned()))
        } else {
            Err(CatalogError::InvalidRuleId(value.to_owned()))
        }
    }
}

impl<'de> Deserialize<'de> for RuleId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

/// Origin of a core requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleKind {
    /// A requirement derived from `MinimumCD`.
    Minimumcd,
    /// A requirement defined by `OpDev`.
    Opdev,
}

/// A decision boundary affected by a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    /// Whether ordinary implementation work may proceed.
    Development,
    /// Whether a change may integrate into trunk.
    Integration,
    /// Whether an artifact may be delivered.
    Delivery,
    /// Whether a conformance claim may be made.
    Compliance,
}

/// A mechanism that can contribute evidence for a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationMethod {
    /// Agent workflow and evidence inspection.
    Agent,
    /// CI configuration or results.
    Ci,
    /// Stored evidence records.
    Evidence,
    /// A project-owned or packaged extension.
    Extension,
    /// Local Git state.
    Git,
    /// Project-manifest declarations.
    Manifest,
    /// Remote repository policy or pipeline state.
    Remote,
}

/// A normative or informative source for a rule.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Human-readable source name.
    pub name: String,
    /// Stable URL or repository-relative path.
    pub url: String,
}

/// A single normative `OpDev` rule.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// Permanent identifier.
    pub id: RuleId,
    /// Requirement origin.
    pub kind: RuleKind,
    /// Short diagnostic title.
    pub title: String,
    /// Normative statement.
    pub statement: String,
    /// Human-readable applicability condition.
    pub applicability: String,
    /// Gates affected by the rule.
    pub gates: Vec<Gate>,
    /// Evidence mechanisms that can verify the rule.
    pub verification: Vec<VerificationMethod>,
    /// Normative or informative sources.
    pub sources: Vec<Source>,
}

/// Versioned collection of core rules.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuleCatalog {
    /// Catalog compatibility version.
    pub catalog_version: u32,
    /// Core rules in stable presentation order.
    pub rules: Vec<Rule>,
}

impl RuleCatalog {
    /// Parses and performs structural validation on a catalog.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] when YAML parsing, rule ID validation, or
    /// structural validation fails.
    pub fn from_yaml(yaml: &str) -> Result<Self, CatalogError> {
        let catalog: Self = serde_saphyr::from_str(yaml)?;
        catalog.validate()?;
        Ok(catalog)
    }

    /// Returns the rule with the requested ID.
    #[must_use]
    pub fn find(&self, id: &RuleId) -> Option<&Rule> {
        self.rules.iter().find(|rule| &rule.id == id)
    }

    fn validate(&self) -> Result<(), CatalogError> {
        if self.rules.is_empty() {
            return Err(CatalogError::Empty);
        }

        let mut ids = HashSet::with_capacity(self.rules.len());
        for rule in &self.rules {
            if !ids.insert(rule.id.clone()) {
                return Err(CatalogError::DuplicateRuleId(rule.id.clone()));
            }
        }

        Ok(())
    }
}

/// Loads the rule catalog embedded in the `OpDev` binary.
///
/// # Errors
///
/// Returns [`CatalogError`] when the embedded catalog cannot be parsed or fails
/// structural validation.
pub fn embedded_catalog() -> Result<RuleCatalog, CatalogError> {
    RuleCatalog::from_yaml(EMBEDDED_CATALOG)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_is_structurally_valid() -> Result<(), Box<dyn std::error::Error>> {
        let catalog = embedded_catalog()?;
        assert_eq!(catalog.catalog_version, 1);
        assert_eq!(catalog.rules.len(), 37);
        Ok(())
    }

    #[test]
    fn rule_ids_reject_invalid_shapes() {
        assert!("OPDEV-TEST-001".parse::<RuleId>().is_ok());
        assert!("opdev-test-001".parse::<RuleId>().is_err());
        assert!("OPDEV-001".parse::<RuleId>().is_err());
        assert!("OPDEV-TEST-1".parse::<RuleId>().is_err());
    }

    #[test]
    fn catalog_matches_its_json_schema() -> Result<(), Box<dyn std::error::Error>> {
        let yaml_value: serde_json::Value = serde_saphyr::from_str(EMBEDDED_CATALOG)?;
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../rules/core.schema.json"))?;
        let validator = jsonschema::validator_for(&schema)?;
        let errors: Vec<_> = validator.iter_errors(&yaml_value).collect();
        assert!(errors.is_empty(), "schema errors: {errors:#?}");
        Ok(())
    }
}
