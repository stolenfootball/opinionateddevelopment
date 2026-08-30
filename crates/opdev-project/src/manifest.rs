use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use opdev_core::{PROJECT_SCHEMA_VERSION, resolve_profile};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const PROJECT_SCHEMA: &str = include_str!("../../../schema/project.schema.json");

/// Errors produced while reading or validating a project contract.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// The project contract could not be read.
    #[error("could not read project contract `{path}`: {source}")]
    Read {
        /// Contract path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },

    /// The project contract could not be written.
    #[error("could not write project contract `{path}`: {source}")]
    Write {
        /// Contract path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },

    /// YAML parsing failed.
    #[error("project contract YAML is invalid: {0}")]
    Yaml(#[from] serde_saphyr::Error),

    /// YAML serialization failed.
    #[error("project contract could not be serialized as YAML: {0}")]
    YamlSerialize(#[from] serde_saphyr::SerializeError),

    /// The bundled JSON Schema could not be parsed.
    #[error("the bundled project schema is invalid: {0}")]
    SchemaDocument(#[from] serde_json::Error),

    /// JSON Schema compilation failed.
    #[error("the bundled project schema could not be compiled: {0}")]
    SchemaCompile(String),

    /// The document violated the project schema.
    #[error("project contract schema validation failed:\n{0}")]
    SchemaValidation(String),

    /// The schema is recognized syntactically but not supported by this CLI.
    #[error("project contract schema {found} is unsupported; this CLI supports schema {supported}")]
    UnsupportedSchema {
        /// Schema found in the project contract.
        found: u32,
        /// Schema supported by the CLI.
        supported: u32,
    },

    /// Cross-field semantics are invalid.
    #[error("project contract is inconsistent: {0}")]
    Semantic(String),
}

/// Complete, versioned `OpDev` project contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifest {
    /// Manifest schema version.
    pub schema: u32,
    /// Project identity and CI topology.
    pub project: Project,
    /// Canonical homes for important project facts.
    pub authorities: BTreeMap<String, AuthorityRef>,
    /// Canonical executable commands.
    pub commands: BTreeMap<String, CommandSpec>,
    /// Applicable quality risks.
    pub quality: Quality,
    /// Testing policy and suites.
    pub testing: Testing,
    /// Artifact, destination, and recovery contract.
    pub delivery: Delivery,
    /// Operational evidence routing.
    #[serde(default, skip_serializing_if = "Operations::is_empty")]
    pub operations: Operations,
    /// Version-pinned assurance profiles.
    pub assurance: Assurance,
    /// Project-specific checks.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
    /// Task-specific authority routing.
    pub context: Context,
}

impl ProjectManifest {
    /// Loads and validates a project contract from disk.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] when the file cannot be read, parsed, validated,
    /// or reconciled semantically.
    pub fn load(path: &Path) -> Result<Self, ManifestError> {
        let yaml = fs::read_to_string(path).map_err(|source| ManifestError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_yaml(&yaml)
    }

    /// Parses and validates a project contract.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] when YAML, JSON Schema, or semantic validation
    /// fails.
    pub fn from_yaml(yaml: &str) -> Result<Self, ManifestError> {
        let value: serde_json::Value = serde_saphyr::from_str(yaml)?;
        validate_schema(&value)?;
        let manifest: Self = serde_saphyr::from_str(yaml)?;
        manifest.validate_semantics()?;
        Ok(manifest)
    }

    /// Serializes and validates a project contract.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] when the in-memory contract is inconsistent or
    /// cannot be serialized.
    pub fn to_yaml(&self) -> Result<String, ManifestError> {
        self.validate_semantics()?;
        let yaml = serde_saphyr::to_string(self)?;
        let value: serde_json::Value = serde_saphyr::from_str(&yaml)?;
        validate_schema(&value)?;
        Ok(yaml)
    }

    /// Writes a new project contract without replacing an existing file.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] when validation, directory creation, or an
    /// atomic new-file write fails.
    pub fn write_new(&self, path: &Path) -> Result<(), ManifestError> {
        let yaml = self.to_yaml()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| ManifestError::Write {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = options.open(path).map_err(|source| ManifestError::Write {
            path: path.to_path_buf(),
            source,
        })?;
        std::io::Write::write_all(&mut file, yaml.as_bytes()).map_err(|source| {
            ManifestError::Write {
                path: path.to_path_buf(),
                source,
            }
        })
    }

    fn validate_semantics(&self) -> Result<(), ManifestError> {
        if self.schema != PROJECT_SCHEMA_VERSION {
            return Err(ManifestError::UnsupportedSchema {
                found: self.schema,
                supported: PROJECT_SCHEMA_VERSION,
            });
        }
        if self.project.trunk.trim() != self.project.trunk
            || self.project.trunk.chars().any(char::is_whitespace)
        {
            return Err(ManifestError::Semantic(
                "project.trunk must be a non-empty Git branch name without whitespace".into(),
            ));
        }

        for (name, command) in &self.commands {
            if command.argv.is_empty() || command.argv.iter().any(String::is_empty) {
                return Err(ManifestError::Semantic(format!(
                    "command `{name}` must contain a non-empty argument vector"
                )));
            }
            if let Some(directory) = &command.working_directory
                && !is_safe_relative_path(directory)
            {
                return Err(ManifestError::Semantic(format!(
                    "command `{name}` working_directory must stay within the project"
                )));
            }
        }

        for (name, authority) in &self.authorities {
            if authority.kind == AuthorityKind::Path && !is_safe_relative_path(&authority.location)
            {
                return Err(ManifestError::Semantic(format!(
                    "authority `{name}` path must stay within the project"
                )));
            }
        }

        if let Some(strategy) = &self.testing.strategy_authority {
            self.require_authority(strategy, "testing.strategy_authority")?;
        }
        for suite in &self.testing.suites {
            self.require_command(&suite.command, &format!("test suite `{}`", suite.id))?;
        }
        if let Some(command) = &self.delivery.recovery.command {
            self.require_command(command, "delivery.recovery")?;
        }
        if let Some(authority) = &self.operations.observability_authority {
            self.require_authority(authority, "operations.observability_authority")?;
        }
        for authority in &self.context.always {
            self.require_authority(authority, "context.always")?;
        }
        for (route, authorities) in &self.context.routes {
            for authority in authorities {
                self.require_authority(authority, &format!("context.routes.{route}"))?;
            }
        }
        for check in &self.extensions.checks {
            self.require_command(&check.command, &format!("extension `{}`", check.id))?;
            if let Some(authority) = &check.authority {
                self.require_authority(authority, &format!("extension `{}`", check.id))?;
            }
        }

        ensure_unique(
            self.testing.suites.iter().map(|suite| suite.id.as_str()),
            "test suite",
        )?;
        ensure_unique(
            self.extensions.checks.iter().map(|check| check.id.as_str()),
            "extension check",
        )?;
        ensure_unique(
            self.assurance
                .profiles
                .iter()
                .map(|profile| profile.name.as_str()),
            "assurance profile",
        )?;
        for profile in &self.assurance.profiles {
            resolve_profile(&profile.name, &profile.version, profile.level.as_deref())
                .map_err(|error| ManifestError::Semantic(error.to_string()))?;
        }

        Ok(())
    }

    fn require_command(&self, name: &str, owner: &str) -> Result<(), ManifestError> {
        if self.commands.contains_key(name) {
            Ok(())
        } else {
            Err(ManifestError::Semantic(format!(
                "{owner} references unknown command `{name}`"
            )))
        }
    }

    fn require_authority(&self, name: &str, owner: &str) -> Result<(), ManifestError> {
        if self.authorities.contains_key(name) {
            Ok(())
        } else {
            Err(ManifestError::Semantic(format!(
                "{owner} references unknown authority `{name}`"
            )))
        }
    }
}

fn validate_schema(value: &serde_json::Value) -> Result<(), ManifestError> {
    let schema: serde_json::Value = serde_json::from_str(PROJECT_SCHEMA)?;
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| ManifestError::SchemaCompile(error.to_string()))?;
    let messages: Vec<_> = validator
        .iter_errors(value)
        .map(|error| format!("- {}: {}", error.instance_path(), error))
        .collect();
    if messages.is_empty() {
        Ok(())
    } else {
        Err(ManifestError::SchemaValidation(messages.join("\n")))
    }
}

fn is_safe_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !path.is_absolute()
        && !value.is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

fn ensure_unique<'a>(
    values: impl IntoIterator<Item = &'a str>,
    label: &str,
) -> Result<(), ManifestError> {
    let mut seen = HashSet::new();
    for value in values {
        if !seen.insert(value) {
            return Err(ManifestError::Semantic(format!(
                "duplicate {label} ID `{value}`"
            )));
        }
    }
    Ok(())
}

/// Project identity and CI topology.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// General software archetype.
    pub kind: ProjectKind,
    /// Single integration trunk.
    pub trunk: String,
    /// CI provider and remote.
    pub ci: CiConfig,
}

/// General software archetype.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    /// Deployed service or API.
    Service,
    /// Web application.
    Web,
    /// Desktop application.
    Desktop,
    /// Mobile application.
    Mobile,
    /// Published library or SDK.
    Library,
    /// Command-line application.
    Cli,
    /// Plugin or extension.
    Plugin,
    /// Firmware image.
    Firmware,
    /// Embedded software.
    Embedded,
    /// Infrastructure definition.
    Infrastructure,
    /// Database or migration project.
    Database,
    /// Data pipeline.
    DataPipeline,
    /// Machine-learning system.
    MachineLearning,
    /// Documentation product.
    Documentation,
    /// Software that does not match a more specific archetype.
    Generic,
}

/// CI configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiConfig {
    /// Detected or declared provider.
    pub provider: CiProvider,
    /// Git remote used for read-only audits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
}

/// Supported CI provider families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CiProvider {
    /// GitHub Actions.
    Github,
    /// GitLab CI.
    Gitlab,
    /// Detected but not first-class provider.
    Other,
    /// No CI provider could be inferred.
    Unconfigured,
}

/// Canonical information source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityRef {
    /// Location type.
    pub kind: AuthorityKind,
    /// Repository-relative path, URL, or tracker identifier.
    pub location: String,
}

/// Canonical information-source type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityKind {
    /// Repository-relative file or directory.
    Path,
    /// External URL.
    Url,
    /// Work-management system.
    Tracker,
}

/// Safe, shell-free canonical command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandSpec {
    /// Executable followed by arguments.
    pub argv: Vec<String>,
    /// Optional repository-relative working directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    /// Optional execution timeout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
}

/// Applicable quality risks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quality {
    /// Risks that drive acceptance and testing objectives.
    pub risks: Vec<QualityRisk>,
}

/// Quality characteristic used for risk-based testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityRisk {
    /// Functional suitability and correctness.
    Functional,
    /// Reliability under expected conditions.
    Reliability,
    /// Security and misuse resistance.
    Security,
    /// Performance and resource efficiency.
    Performance,
    /// Compatibility with declared consumers and dependencies.
    Compatibility,
    /// Maintainability and change safety.
    Maintainability,
    /// Usability.
    Usability,
    /// Accessibility.
    Accessibility,
    /// Safety.
    Safety,
    /// Data correctness and preservation.
    DataIntegrity,
    /// Recovery from failed change or operation.
    Recoverability,
    /// Achievement of intended purpose.
    Effectiveness,
}

/// Testing policy and executable suites.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Testing {
    /// Authority containing a longer testing strategy, when needed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy_authority: Option<String>,
    /// Required behavior for changed functionality.
    pub change_tests: ChangeTests,
    /// Required behavior for escaped defects.
    pub escaped_defect_regressions: EscapedDefectRegressions,
    /// Flake handling policy.
    pub flake_policy: FlakePolicy,
    /// Coverage policy.
    pub coverage: Coverage,
    /// Executable test suites.
    pub suites: Vec<TestSuite>,
}

/// Behavioral-change testing requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeTests {
    /// Behavioral changes require automated verification.
    Required,
}

/// Escaped-defect regression requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscapedDefectRegressions {
    /// Regression protection is required unless specifically justified.
    RequiredOrJustified,
}

/// Mandatory treatment of flaky qualification tests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlakePolicy {
    /// Retries remain visible in evidence.
    pub retries_visible: bool,
    /// Quarantine requires ownership, tracked work, and expiry.
    pub quarantine_requires_owner_issue_expiry: bool,
}

/// Code-coverage evidence policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    /// How coverage contributes evidence.
    pub mode: CoverageMode,
    /// Project-selected percentage when the mode uses a threshold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
}

/// Supported coverage strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageMode {
    /// Coverage is not yet configured.
    Unconfigured,
    /// Coverage is reported for risk inspection but not gated.
    Report,
    /// Material regressions are blocked.
    NonRegression,
    /// Changed code has a project-selected threshold.
    ChangedCodeThreshold,
    /// Critical modules define their own thresholds.
    CriticalModuleThresholds,
}

/// Executable test suite and its pipeline stages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestSuite {
    /// Stable project-local suite ID.
    pub id: String,
    /// Canonical command key.
    pub command: String,
    /// Stages that execute this suite.
    pub stages: Vec<TestStage>,
}

/// Pipeline or workflow stage for a test suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestStage {
    /// Local development.
    Local,
    /// Before integration.
    PreMerge,
    /// On integrated trunk.
    PostMerge,
    /// Artifact packaging.
    Package,
    /// Delivery qualification.
    Delivery,
    /// Recovery qualification.
    Recovery,
    /// Scheduled specialized qualification.
    Scheduled,
    /// Effectiveness evaluation.
    Evaluation,
}

/// Delivery contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delivery {
    /// Whether delivery is currently qualified.
    pub status: DeliveryStatus,
    /// Consumer-facing delivery action.
    pub mode: DeliveryMode,
    /// Artifact identity strategy.
    pub artifact: Artifact,
    /// Qualification and consumer-facing environments.
    pub environments: Vec<Environment>,
    /// Automated recovery strategy.
    pub recovery: Recovery,
}

/// Delivery configuration state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    /// Delivery requirements are configured.
    Configured,
    /// Brownfield delivery gaps remain.
    MigrationRequired,
}

/// Consumer-facing delivery action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryMode {
    /// Deploy to an environment.
    Deploy,
    /// Publish to a registry or store.
    Publish,
    /// Install on a consumer system.
    Install,
    /// Flash a device.
    Flash,
    /// Apply infrastructure or database state.
    Apply,
    /// Render a site or documentation product.
    Render,
    /// Publish release assets.
    Release,
}

/// Deliverable artifact declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    /// Project-defined artifact kind.
    pub kind: String,
    /// Path, package coordinate, image name, or other stable locator.
    pub locator: String,
}

/// Delivery or qualification environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    /// Project-defined environment name.
    pub name: String,
    /// Whether it represents material consumer-facing risks.
    pub production_like: bool,
}

/// Automated recovery declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recovery {
    /// Recovery method.
    pub strategy: RecoveryStrategy,
    /// Canonical recovery command, when configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

/// Recovery strategy appropriate to the artifact and state model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryStrategy {
    /// Roll back the current deployment.
    Rollback,
    /// Redeploy the previous immutable artifact.
    RedeployPrevious,
    /// Disable the affected capability.
    Disable,
    /// Restore data or state.
    Restore,
    /// Apply a previously tested forward migration.
    RollForward,
    /// Deliver a focused forward fix.
    ForwardFix,
    /// Recovery is a known migration gap.
    Unconfigured,
}

/// Operational evidence routing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operations {
    /// Command or evidence selector used after delivery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_evidence: Option<String>,
    /// Authority containing observability and service objectives.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observability_authority: Option<String>,
}

impl Operations {
    fn is_empty(&self) -> bool {
        self.health_evidence.is_none() && self.observability_authority.is_none()
    }
}

/// Version-pinned assurance selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assurance {
    /// Selected profiles.
    pub profiles: Vec<Profile>,
}

/// One assurance profile selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Profile name.
    pub name: String,
    /// Exact profile version.
    pub version: String,
    /// Optional level within the profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
}

/// Project-specific extension checks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extensions {
    /// Checks added by the project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<ExtensionCheck>,
}

impl Extensions {
    fn is_empty(&self) -> bool {
        self.checks.is_empty()
    }
}

/// Project-owned extension check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionCheck {
    /// Stable project-local check ID.
    pub id: String,
    /// Lifecycle stage.
    pub stage: ExtensionStage,
    /// Canonical command key.
    pub command: String,
    /// Whether failure blocks its stage.
    pub blocking: bool,
    /// Optional authority describing the invariant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authority: Option<String>,
    /// Optional timeout override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
}

/// Lifecycle stage for an extension check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionStage {
    /// Work specification.
    Specify,
    /// Design.
    Design,
    /// General verification.
    Verify,
    /// Before integration.
    PreMerge,
    /// On integrated trunk.
    PostMerge,
    /// Packaging.
    Package,
    /// Delivery.
    Deliver,
    /// Post-delivery smoke testing.
    Smoke,
    /// Recovery.
    Recover,
    /// Effectiveness evaluation.
    Evaluate,
}

/// Task-specific authority routing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    /// Authorities read before every substantive task.
    pub always: Vec<String>,
    /// Task category to additional authorities.
    pub routes: BTreeMap<String, Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_manifest() -> ProjectManifest {
        let mut authorities = BTreeMap::new();
        authorities.insert(
            "contracts".into(),
            AuthorityRef {
                kind: AuthorityKind::Path,
                location: "spec".into(),
            },
        );
        let mut commands = BTreeMap::new();
        commands.insert(
            "check".into(),
            CommandSpec {
                argv: vec!["cargo".into(), "test".into()],
                working_directory: None,
                timeout_seconds: Some(900),
            },
        );
        ProjectManifest {
            schema: 1,
            project: Project {
                kind: ProjectKind::Cli,
                trunk: "main".into(),
                ci: CiConfig {
                    provider: CiProvider::Gitlab,
                    remote: Some("git@gitlab.com:example/project.git".into()),
                },
            },
            authorities,
            commands,
            quality: Quality {
                risks: vec![QualityRisk::Functional],
            },
            testing: Testing {
                strategy_authority: None,
                change_tests: ChangeTests::Required,
                escaped_defect_regressions: EscapedDefectRegressions::RequiredOrJustified,
                flake_policy: FlakePolicy {
                    retries_visible: true,
                    quarantine_requires_owner_issue_expiry: true,
                },
                coverage: Coverage {
                    mode: CoverageMode::Unconfigured,
                    threshold: None,
                },
                suites: vec![TestSuite {
                    id: "check".into(),
                    command: "check".into(),
                    stages: vec![TestStage::PreMerge, TestStage::PostMerge],
                }],
            },
            delivery: Delivery {
                status: DeliveryStatus::MigrationRequired,
                mode: DeliveryMode::Release,
                artifact: Artifact {
                    kind: "binary".into(),
                    locator: "cargo:example".into(),
                },
                environments: Vec::new(),
                recovery: Recovery {
                    strategy: RecoveryStrategy::Unconfigured,
                    command: None,
                },
            },
            operations: Operations::default(),
            assurance: Assurance {
                profiles: vec![Profile {
                    name: "opdev-core".into(),
                    version: "1".into(),
                    level: None,
                }],
            },
            extensions: Extensions::default(),
            context: Context {
                always: vec!["contracts".into()],
                routes: BTreeMap::new(),
            },
        }
    }

    #[test]
    fn manifest_round_trips_through_yaml_and_schema() -> Result<(), Box<dyn std::error::Error>> {
        let manifest = minimal_manifest();
        let yaml = manifest.to_yaml()?;
        assert_eq!(ProjectManifest::from_yaml(&yaml)?, manifest);
        Ok(())
    }

    #[test]
    fn unknown_command_reference_is_rejected() {
        let mut manifest = minimal_manifest();
        manifest.testing.suites[0].command = "missing".into();
        assert!(matches!(
            manifest.to_yaml(),
            Err(ManifestError::Semantic(message)) if message.contains("unknown command")
        ));
    }

    #[test]
    fn paths_cannot_escape_the_project() {
        let mut manifest = minimal_manifest();
        if let Some(authority) = manifest.authorities.get_mut("contracts") {
            authority.location = "../outside".into();
        }
        assert!(matches!(
            manifest.to_yaml(),
            Err(ManifestError::Semantic(message)) if message.contains("stay within")
        ));
    }

    #[test]
    fn unknown_assurance_profile_versions_are_rejected() {
        let mut manifest = minimal_manifest();
        manifest.assurance.profiles[0].version = "latest".into();
        assert!(matches!(
            manifest.to_yaml(),
            Err(ManifestError::Semantic(message)) if message.contains("not supported")
        ));
    }
}
