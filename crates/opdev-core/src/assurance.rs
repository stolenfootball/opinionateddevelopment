use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{RuleCatalog, RuleId, embedded_catalog};

const PROFILE_DOCUMENTS: &[&str] = &[
    include_str!("../../../profiles/opdev-core/1.yaml"),
    include_str!("../../../profiles/nist-ssdf-derived/1.1.yaml"),
    include_str!("../../../profiles/slsa-build-provenance/1.2.yaml"),
    include_str!("../../../profiles/cyclonedx-sbom/1.5.yaml"),
    include_str!("../../../profiles/openssf-osps-baseline-derived/2026.02.19.yaml"),
];

/// Errors encountered while loading or selecting assurance profiles.
#[derive(Debug, Error)]
pub enum ProfileError {
    /// A bundled YAML document is malformed.
    #[error("an embedded assurance profile is invalid YAML: {0}")]
    Yaml(#[from] serde_saphyr::Error),
    /// The core catalog required for mapping validation is malformed.
    #[error("the embedded rule catalog could not be loaded: {0}")]
    Catalog(#[from] crate::CatalogError),
    /// A profile name and version are not bundled with this release.
    #[error(
        "assurance profile `{name}` version `{version}` is not supported by this OpDev release"
    )]
    Unsupported {
        /// Requested profile name.
        name: String,
        /// Requested exact version.
        version: String,
    },
    /// A selected level is absent from the exact profile version.
    #[error("assurance profile `{name}` version `{version}` does not define level `{level}`")]
    UnsupportedLevel {
        /// Requested profile name.
        name: String,
        /// Requested exact version.
        version: String,
        /// Requested level.
        level: String,
    },
    /// A profile contains a duplicate requirement ID.
    #[error("profile `{profile}` contains duplicate requirement `{requirement}`")]
    DuplicateRequirement {
        /// Profile identity.
        profile: String,
        /// Duplicate requirement ID.
        requirement: String,
    },
    /// A profile maps to an unknown stable core rule.
    #[error("profile `{profile}` requirement `{requirement}` maps unknown rule `{rule}`")]
    UnknownRule {
        /// Profile identity.
        profile: String,
        /// Requirement identity.
        requirement: String,
        /// Unknown rule identity.
        rule: RuleId,
    },
}

/// Intended interpretation of a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileStatus {
    /// The profile is the normative `OpDev` baseline.
    Normative,
    /// The profile is an informative mapping to an external framework.
    DerivedMapping,
    /// The profile describes an evidence format rather than conformance.
    EvidenceFormat,
}

/// Degree to which current `OpDev` rules cover a mapped requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementCoverage {
    /// The mapped `OpDev` rules address the requirement as written.
    Full,
    /// The mapped rules contribute evidence but do not establish the whole requirement.
    Partial,
    /// `OpDev` does not currently establish the requirement.
    Gap,
}

/// Versioned external or internal source for a profile.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSource {
    /// Source name.
    pub name: String,
    /// Exact source version.
    pub version: String,
    /// Stable authoritative URL or repository path.
    pub url: String,
}

/// One mapped requirement within an assurance profile.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileRequirement {
    /// Requirement identifier in the source profile.
    pub id: String,
    /// Concise requirement title.
    pub title: String,
    /// Mapping coverage, never inferred from the number of linked rules.
    pub coverage: RequirementCoverage,
    /// Stable `OpDev` rules that can contribute evidence.
    #[serde(default)]
    pub mapped_rules: Vec<RuleId>,
    /// Important qualification or missing evidence.
    pub limitation: String,
}

/// A bundled, exact-version assurance mapping.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssuranceProfile {
    /// Profile document schema version.
    pub schema: u32,
    /// Stable profile name.
    pub name: String,
    /// Exact profile version.
    pub version: String,
    /// Human-readable title.
    pub title: String,
    /// Intended interpretation.
    pub status: ProfileStatus,
    /// Explicit statement of what selecting the profile does and does not claim.
    pub claim: String,
    /// Versioned source.
    pub source: ProfileSource,
    /// Optional exact levels understood by this mapping.
    #[serde(default)]
    pub levels: Vec<String>,
    /// Mapped requirements.
    pub requirements: Vec<ProfileRequirement>,
}

impl AssuranceProfile {
    fn validate(&self, catalog: &RuleCatalog) -> Result<(), ProfileError> {
        let identity = format!("{}@{}", self.name, self.version);
        let mut requirements = HashSet::new();
        for requirement in &self.requirements {
            if !requirements.insert(requirement.id.as_str()) {
                return Err(ProfileError::DuplicateRequirement {
                    profile: identity,
                    requirement: requirement.id.clone(),
                });
            }
            for rule in &requirement.mapped_rules {
                if catalog.find(rule).is_none() {
                    return Err(ProfileError::UnknownRule {
                        profile: identity.clone(),
                        requirement: requirement.id.clone(),
                        rule: rule.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}

/// Loads all profiles embedded in this `OpDev` release.
///
/// # Errors
///
/// Returns [`ProfileError`] when a document is malformed or its rule mappings
/// are inconsistent with the embedded catalog.
pub fn embedded_profiles() -> Result<Vec<AssuranceProfile>, ProfileError> {
    let catalog = embedded_catalog()?;
    PROFILE_DOCUMENTS
        .iter()
        .map(|document| {
            let profile: AssuranceProfile = serde_saphyr::from_str(document)?;
            profile.validate(&catalog)?;
            Ok(profile)
        })
        .collect()
}

/// Resolves one exact built-in profile and optional level.
///
/// # Errors
///
/// Returns [`ProfileError`] when the profile, version, or level is unsupported.
pub fn resolve_profile(
    name: &str,
    version: &str,
    level: Option<&str>,
) -> Result<AssuranceProfile, ProfileError> {
    let profile = embedded_profiles()?
        .into_iter()
        .find(|profile| profile.name == name && profile.version == version)
        .ok_or_else(|| ProfileError::Unsupported {
            name: name.to_owned(),
            version: version.to_owned(),
        })?;
    if let Some(level) = level
        && !profile.levels.iter().any(|candidate| candidate == level)
    {
        return Err(ProfileError::UnsupportedLevel {
            name: name.to_owned(),
            version: version.to_owned(),
            level: level.to_owned(),
        });
    }
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_profiles_are_valid_and_unique() -> Result<(), Box<dyn std::error::Error>> {
        let profiles = embedded_profiles()?;
        let identities: HashSet<_> = profiles
            .iter()
            .map(|profile| (&profile.name, &profile.version))
            .collect();
        assert_eq!(profiles.len(), 5);
        assert_eq!(identities.len(), profiles.len());
        Ok(())
    }

    #[test]
    fn profiles_match_their_json_schema() -> Result<(), Box<dyn std::error::Error>> {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../schema/profile.schema.json"))?;
        let validator = jsonschema::validator_for(&schema)?;
        for document in PROFILE_DOCUMENTS {
            let value: serde_json::Value = serde_saphyr::from_str(document)?;
            let errors: Vec<_> = validator.iter_errors(&value).collect();
            assert!(errors.is_empty(), "schema errors: {errors:#?}");
        }
        Ok(())
    }

    #[test]
    fn profile_versions_and_levels_are_exact() {
        assert!(resolve_profile("nist-ssdf-derived", "1.1", Some("baseline")).is_ok());
        assert!(resolve_profile("nist-ssdf-derived", "latest", None).is_err());
        assert!(resolve_profile("nist-ssdf-derived", "1.1", Some("complete")).is_err());
    }
}
