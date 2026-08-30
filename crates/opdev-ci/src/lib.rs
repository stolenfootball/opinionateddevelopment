//! Extensible local CI configuration generation and inspection.

#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};

use opdev_core::{Evidence, Outcome};
use opdev_project::CiProvider;
use thiserror::Error;

const GITHUB_TEMPLATE: &str = include_str!("../../../templates/ci/github.yml");
const GITLAB_TEMPLATE: &str = include_str!("../../../templates/ci/gitlab.yml");

/// Data used to render a provider configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateContext {
    /// Exact `OpDev` CLI release.
    pub opdev_version: String,
    /// Declared integration trunk.
    pub trunk: String,
}

/// Result for one locally inspectable CI capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    /// Exhaustive result outcome.
    pub outcome: Outcome,
    /// Evidence supporting the result.
    pub evidence: Vec<Evidence>,
    /// Missing capability or parser diagnostic.
    pub diagnostic: Option<String>,
}

/// Local CI findings mapped to core rule evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiInspection {
    /// Provider configuration exists and is structurally usable.
    pub configuration: Capability,
    /// Change pipelines invoke the `OpDev` integration gate.
    pub pre_merge: Capability,
    /// Trunk pipelines invoke the `OpDev` integration gate.
    pub post_merge: Capability,
    /// The pinned CLI archive is checksum-verified before execution.
    pub integrity: Capability,
}

/// CI adapter failures.
#[derive(Debug, Error)]
pub enum CiError {
    /// The provider is not first-class in this release.
    #[error("CI provider `{0:?}` has no first-class adapter")]
    Unsupported(CiProvider),
    /// A template value would be unsafe or ambiguous.
    #[error("invalid CI template value: {0}")]
    InvalidTemplateValue(String),
    /// A CI file could not be read.
    #[error("could not read CI configuration `{path}`: {source}")]
    Read {
        /// Configuration path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// A new CI file could not be written.
    #[error("could not write CI configuration `{path}`: {source}")]
    Write {
        /// Configuration path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
}

/// Provider boundary for future first-class CI integrations.
pub trait CiAdapter: Sync {
    /// Provider represented by this adapter.
    fn provider(&self) -> CiProvider;
    /// Repository-relative canonical configuration path.
    fn configuration_path(&self) -> &'static str;
    /// Renders a secure baseline configuration.
    ///
    /// # Errors
    ///
    /// Returns [`CiError`] when a template value is unsafe or cannot be encoded.
    fn render(&self, context: &TemplateContext) -> Result<String, CiError>;
    /// Inspects an existing local configuration without executing it.
    ///
    /// # Errors
    ///
    /// Returns [`CiError`] when the local configuration cannot be read.
    fn inspect(&self, root: &Path) -> Result<CiInspection, CiError>;
}

struct GithubAdapter;
struct GitlabAdapter;

static GITHUB: GithubAdapter = GithubAdapter;
static GITLAB: GitlabAdapter = GitlabAdapter;

/// Returns the first-class adapter for a provider.
///
/// # Errors
///
/// Returns [`CiError::Unsupported`] for providers intentionally left to future
/// adapter packages.
pub fn adapter_for(provider: CiProvider) -> Result<&'static dyn CiAdapter, CiError> {
    match provider {
        CiProvider::Github => Ok(&GITHUB),
        CiProvider::Gitlab => Ok(&GITLAB),
        CiProvider::Other | CiProvider::Unconfigured => Err(CiError::Unsupported(provider)),
    }
}

/// Writes a rendered configuration without replacing an existing file.
///
/// # Errors
///
/// Returns [`CiError`] when template rendering, directory creation, or the
/// create-new write fails.
pub fn write_new(
    adapter: &dyn CiAdapter,
    root: &Path,
    context: &TemplateContext,
) -> Result<PathBuf, CiError> {
    let path = root.join(adapter.configuration_path());
    let rendered = adapter.render(context)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| CiError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options.open(&path).map_err(|source| CiError::Write {
        path: path.clone(),
        source,
    })?;
    std::io::Write::write_all(&mut file, rendered.as_bytes()).map_err(|source| CiError::Write {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

impl CiAdapter for GithubAdapter {
    fn provider(&self) -> CiProvider {
        CiProvider::Github
    }

    fn configuration_path(&self) -> &'static str {
        ".github/workflows/opdev.yml"
    }

    fn render(&self, context: &TemplateContext) -> Result<String, CiError> {
        render_template(GITHUB_TEMPLATE, context)
    }

    fn inspect(&self, root: &Path) -> Result<CiInspection, CiError> {
        inspect_file(
            &root.join(self.configuration_path()),
            &ProviderRequirements {
                pre_merge: &["pull_request", "opdev check --ci"],
                post_merge: &["push", "opdev check --ci"],
                integrity: &["SHA256SUMS", "sha256sum -c"],
            },
        )
    }
}

impl CiAdapter for GitlabAdapter {
    fn provider(&self) -> CiProvider {
        CiProvider::Gitlab
    }

    fn configuration_path(&self) -> &'static str {
        ".gitlab-ci.yml"
    }

    fn render(&self, context: &TemplateContext) -> Result<String, CiError> {
        render_template(GITLAB_TEMPLATE, context)
    }

    fn inspect(&self, root: &Path) -> Result<CiInspection, CiError> {
        inspect_file(
            &root.join(self.configuration_path()),
            &ProviderRequirements {
                pre_merge: &["merge_request_event", "opdev check --ci"],
                post_merge: &["CI_DEFAULT_BRANCH", "opdev check --ci"],
                integrity: &["SHA256SUMS", "sha256sum -c"],
            },
        )
    }
}

fn render_template(template: &str, context: &TemplateContext) -> Result<String, CiError> {
    if !valid_version(&context.opdev_version) {
        return Err(CiError::InvalidTemplateValue(
            "opdev_version must contain only ASCII letters, digits, dots, plus signs, or hyphens"
                .into(),
        ));
    }
    if context.trunk.trim().is_empty() {
        return Err(CiError::InvalidTemplateValue(
            "trunk must not be empty".into(),
        ));
    }
    let version = serde_json::to_string(&context.opdev_version)
        .map_err(|error| CiError::InvalidTemplateValue(error.to_string()))?;
    let trunk = serde_json::to_string(&context.trunk)
        .map_err(|error| CiError::InvalidTemplateValue(error.to_string()))?;
    Ok(template
        .replace("{{VERSION_JSON}}", &version)
        .replace("{{TRUNK_JSON}}", &trunk))
}

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".+-".contains(character))
}

struct ProviderRequirements {
    pre_merge: &'static [&'static str],
    post_merge: &'static [&'static str],
    integrity: &'static [&'static str],
}

fn inspect_file(path: &Path, requirements: &ProviderRequirements) -> Result<CiInspection, CiError> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(missing_inspection(path));
        }
        Err(source) => {
            return Err(CiError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let parsed = serde_saphyr::from_str::<serde_json::Value>(&content);
    let Ok(parsed) = parsed else {
        let diagnostic = parsed.err().map(|error| error.to_string());
        let error = Capability {
            outcome: Outcome::Error,
            evidence: Vec::new(),
            diagnostic,
        };
        return Ok(CiInspection {
            configuration: error.clone(),
            pre_merge: error.clone(),
            post_merge: error.clone(),
            integrity: error,
        });
    };
    let searchable = serde_json::to_string(&parsed).map_err(|source| CiError::Read {
        path: path.to_path_buf(),
        source: std::io::Error::other(source),
    })?;
    let configuration = passed(path, "CI configuration is valid YAML");
    Ok(CiInspection {
        configuration,
        pre_merge: required_strings(path, &searchable, requirements.pre_merge),
        post_merge: required_strings(path, &searchable, requirements.post_merge),
        integrity: required_strings(path, &searchable, requirements.integrity),
    })
}

fn missing_inspection(path: &Path) -> CiInspection {
    let missing = Capability {
        outcome: Outcome::MigrationRequired,
        evidence: Vec::new(),
        diagnostic: Some(format!(
            "CI configuration `{}` does not exist",
            path.display()
        )),
    };
    CiInspection {
        configuration: missing.clone(),
        pre_merge: missing.clone(),
        post_merge: missing.clone(),
        integrity: missing,
    }
}

fn required_strings(path: &Path, searchable: &str, required: &[&str]) -> Capability {
    let missing: Vec<_> = required
        .iter()
        .filter(|needle| !searchable.contains(**needle))
        .copied()
        .collect();
    if missing.is_empty() {
        passed(path, &format!("CI declares {}", required.join(" and ")))
    } else {
        Capability {
            outcome: Outcome::Failed,
            evidence: Vec::new(),
            diagnostic: Some(format!(
                "CI configuration is missing: {}",
                missing.join(", ")
            )),
        }
    }
}

fn passed(path: &Path, summary: &str) -> Capability {
    Capability {
        outcome: Outcome::Passed,
        evidence: vec![Evidence {
            kind: "ci".into(),
            summary: summary.into(),
            location: Some(path.display().to_string()),
        }],
        diagnostic: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> TemplateContext {
        TemplateContext {
            opdev_version: "0.1.0".into(),
            trunk: "main".into(),
        }
    }

    #[test]
    fn generated_configs_parse_and_satisfy_local_inspection()
    -> Result<(), Box<dyn std::error::Error>> {
        for provider in [CiProvider::Github, CiProvider::Gitlab] {
            let directory = tempfile::tempdir()?;
            let adapter = adapter_for(provider)?;
            write_new(adapter, directory.path(), &context())?;
            let inspection = adapter.inspect(directory.path())?;
            assert_eq!(inspection.configuration.outcome, Outcome::Passed);
            assert_eq!(inspection.pre_merge.outcome, Outcome::Passed);
            assert_eq!(inspection.post_merge.outcome, Outcome::Passed);
            assert_eq!(inspection.integrity.outcome, Outcome::Passed);
        }
        Ok(())
    }

    #[test]
    fn generation_refuses_to_replace_existing_ci() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let adapter = adapter_for(CiProvider::Gitlab)?;
        write_new(adapter, directory.path(), &context())?;
        assert!(write_new(adapter, directory.path(), &context()).is_err());
        Ok(())
    }

    #[test]
    fn missing_configuration_is_a_migration_not_a_pass() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let inspection = adapter_for(CiProvider::Github)?.inspect(directory.path())?;
        assert_eq!(inspection.configuration.outcome, Outcome::MigrationRequired);
        Ok(())
    }
}
