use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use opdev_core::PROJECT_SCHEMA_VERSION;
use thiserror::Error;

use crate::manifest::{
    Artifact, Assurance, AuthorityKind, AuthorityRef, ChangeTests, CiConfig, CiProvider,
    CommandSpec, Context, Coverage, CoverageMode, Delivery, DeliveryMode, DeliveryStatus,
    EscapedDefectRegressions, Extensions, FlakePolicy, Operations, Profile, Project, ProjectKind,
    ProjectManifest, Quality, QualityRisk, Recovery, RecoveryStrategy, TestStage, TestSuite,
    Testing,
};

/// Repository discovery failure.
#[derive(Debug, Error)]
pub enum DiscoveryError {
    /// The requested root does not exist or cannot be resolved.
    #[error("could not resolve project root `{path}`: {source}")]
    Root {
        /// Requested path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },

    /// No repository root could be found.
    #[error("`{0}` is not inside a Git repository")]
    NotRepository(PathBuf),
}

/// Read-only facts and a proposed project contract.
#[derive(Debug, Clone)]
pub struct Discovery {
    /// Resolved repository root.
    pub root: PathBuf,
    /// Proposed project contract.
    pub manifest: ProjectManifest,
    /// Material gaps or ambiguities requiring developer attention.
    pub warnings: Vec<String>,
    /// Files that contributed to inference.
    pub evidence: Vec<String>,
}

/// Discovers a repository and proposes an `OpDev` project contract without
/// executing repository-controlled commands.
///
/// Fixed `git` metadata queries may run, but detected setup, build, test, and
/// delivery commands are never executed.
///
/// # Errors
///
/// Returns [`DiscoveryError`] when the path cannot be resolved or is not inside
/// a Git repository.
pub fn discover(start: &Path) -> Result<Discovery, DiscoveryError> {
    let start = start
        .canonicalize()
        .map_err(|source| DiscoveryError::Root {
            path: start.to_path_buf(),
            source,
        })?;
    let root =
        find_repository_root(&start).ok_or_else(|| DiscoveryError::NotRepository(start.clone()))?;

    let remote = git_output(&root, &["config", "--get", "remote.origin.url"]);
    let trunk = discover_trunk(&root);
    let provider = discover_provider(&root, remote.as_deref());
    let (kind, mut evidence) = discover_kind(&root);
    let mut authorities = discover_authorities(&root, remote.as_deref());
    let commands = discover_commands(&root, &mut evidence);
    let testing = discover_testing(&commands, &authorities);

    if authorities.is_empty() {
        authorities.insert(
            "implementation".into(),
            AuthorityRef {
                kind: AuthorityKind::Path,
                location: ".".into(),
            },
        );
    }

    let context = discover_context(&authorities);
    let delivery = infer_delivery(kind);
    let mut warnings = Vec::new();
    if provider == CiProvider::Unconfigured {
        warnings.push(
            "No CI provider was detected; MinimumCD CI requirements remain a migration gap.".into(),
        );
    }
    if commands.is_empty() {
        warnings.push("No canonical verification command was inferred.".into());
    }
    warnings.push(
        "Delivery and recovery remain migration_required until the developer confirms the artifact, destination, and automated recovery path."
            .into(),
    );

    Ok(Discovery {
        root,
        manifest: ProjectManifest {
            schema: PROJECT_SCHEMA_VERSION,
            project: Project {
                kind,
                trunk,
                ci: CiConfig { provider, remote },
            },
            authorities,
            commands,
            quality: Quality {
                risks: default_quality_risks(kind),
            },
            testing,
            delivery,
            operations: Operations::default(),
            assurance: Assurance {
                profiles: vec![
                    Profile {
                        name: "opdev-core".into(),
                        version: "1".into(),
                        level: None,
                    },
                    Profile {
                        name: "nist-ssdf-derived".into(),
                        version: "1.1".into(),
                        level: Some("baseline".into()),
                    },
                ],
            },
            extensions: Extensions::default(),
            context,
        },
        warnings,
        evidence,
    })
}

fn find_repository_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|candidate| candidate.join(".git").exists())
        .map(Path::to_path_buf)
}

fn git_output(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn discover_trunk(root: &Path) -> String {
    if let Some(reference) = git_output(
        root,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) && let Some((_, branch)) = reference.split_once('/')
    {
        return branch.to_owned();
    }
    for candidate in ["main", "master", "trunk"] {
        if root.join(".git/refs/heads").join(candidate).exists() {
            return candidate.into();
        }
    }
    "main".into()
}

fn discover_provider(root: &Path, remote: Option<&str>) -> CiProvider {
    if root.join(".gitlab-ci.yml").is_file() || remote.is_some_and(|value| value.contains("gitlab"))
    {
        CiProvider::Gitlab
    } else if root.join(".github/workflows").is_dir()
        || remote.is_some_and(|value| value.contains("github"))
    {
        CiProvider::Github
    } else if root.join("Jenkinsfile").is_file()
        || root.join("azure-pipelines.yml").is_file()
        || root.join(".circleci/config.yml").is_file()
    {
        CiProvider::Other
    } else {
        CiProvider::Unconfigured
    }
}

fn discover_kind(root: &Path) -> (ProjectKind, Vec<String>) {
    if root.join(".codex-plugin").is_dir()
        || root.join(".claude-plugin").is_dir()
        || root.join("plugins").is_dir()
    {
        return (
            ProjectKind::Plugin,
            vec!["plugin manifests or plugin directory".into()],
        );
    }
    if root.join("Cargo.toml").is_file() {
        let cargo = fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
        if cargo.contains("[[bin]]") || root.join("crates/opdev-cli").is_dir() {
            return (ProjectKind::Cli, vec!["Cargo binary workspace".into()]);
        }
        return (ProjectKind::Library, vec!["Cargo package".into()]);
    }
    if root.join("package.json").is_file() {
        let package = fs::read_to_string(root.join("package.json")).unwrap_or_default();
        if ["next", "react", "vue", "svelte", "angular"]
            .iter()
            .any(|framework| package.contains(&format!("\"{framework}\"")))
        {
            return (
                ProjectKind::Web,
                vec!["web framework in package.json".into()],
            );
        }
        return (ProjectKind::Generic, vec!["package.json".into()]);
    }
    if root.join("pyproject.toml").is_file() {
        return (ProjectKind::Library, vec!["Python project metadata".into()]);
    }
    if root.join("go.mod").is_file() {
        return (ProjectKind::Library, vec!["Go module".into()]);
    }
    if root.join("main.tf").is_file() || root.join("terraform").is_dir() {
        return (
            ProjectKind::Infrastructure,
            vec!["Terraform configuration".into()],
        );
    }
    if root.join("mkdocs.yml").is_file() || root.join("docusaurus.config.js").is_file() {
        return (
            ProjectKind::Documentation,
            vec!["documentation build configuration".into()],
        );
    }
    (ProjectKind::Generic, Vec::new())
}

fn discover_authorities(root: &Path, remote: Option<&str>) -> BTreeMap<String, AuthorityRef> {
    let mut authorities = BTreeMap::new();
    if let Some(remote) = remote
        && let Some(tracker) = tracker_url(remote)
    {
        authorities.insert(
            "work".into(),
            AuthorityRef {
                kind: AuthorityKind::Tracker,
                location: tracker,
            },
        );
    }
    for (name, candidates) in [
        (
            "architecture",
            ["architecture", "docs/architecture", "spec"],
        ),
        ("decisions", ["decisions", "docs/decisions", "adr"]),
        ("contracts", ["contracts", "spec", "rules"]),
        ("implementation", ["crates", "src", "app"]),
        ("testing", ["tests", "test", "spec"]),
        (
            "delivery",
            [".github/workflows", ".gitlab-ci.yml", "delivery"],
        ),
        ("operations", ["operations", "runbooks", "ops"]),
        ("evaluation", ["evaluation", "evals", "benchmarks"]),
    ] {
        if let Some(candidate) = candidates
            .iter()
            .find(|candidate| root.join(candidate).exists())
        {
            authorities.insert(
                name.into(),
                AuthorityRef {
                    kind: AuthorityKind::Path,
                    location: (*candidate).into(),
                },
            );
        }
    }
    authorities
}

fn tracker_url(remote: &str) -> Option<String> {
    let normalized = if let Some(path) = remote.strip_prefix("git@gitlab.com:") {
        format!("https://gitlab.com/{path}")
    } else if let Some(path) = remote.strip_prefix("git@github.com:") {
        format!("https://github.com/{path}")
    } else {
        remote.to_owned()
    };
    let repository = normalized.trim_end_matches(".git");
    if repository.contains("gitlab") {
        Some(format!("{repository}/-/issues"))
    } else if repository.contains("github") {
        Some(format!("{repository}/issues"))
    } else {
        None
    }
}

fn discover_commands(root: &Path, evidence: &mut Vec<String>) -> BTreeMap<String, CommandSpec> {
    let mut commands = BTreeMap::new();
    if root.join("Cargo.toml").is_file() {
        evidence.push("Cargo.toml canonical command inference".into());
        commands.insert(
            "setup".into(),
            command(&["cargo", "fetch", "--locked"], 900),
        );
        commands.insert(
            "format".into(),
            command(&["cargo", "fmt", "--all", "--", "--check"], 300),
        );
        commands.insert(
            "lint".into(),
            command(
                &[
                    "cargo",
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--all-features",
                    "--",
                    "-D",
                    "warnings",
                ],
                900,
            ),
        );
        commands.insert(
            "check".into(),
            command(&["cargo", "test", "--workspace", "--all-features"], 1800),
        );
        commands.insert(
            "package".into(),
            command(&["cargo", "build", "--release", "--locked"], 1800),
        );
    } else if root.join("package.json").is_file() {
        evidence.push("package.json canonical command inference".into());
        let package = fs::read_to_string(root.join("package.json"))
            .ok()
            .and_then(|source| serde_json::from_str::<serde_json::Value>(&source).ok());
        let has_script = |name: &str| {
            package
                .as_ref()
                .and_then(|value| value.get("scripts"))
                .and_then(|scripts| scripts.get(name))
                .and_then(serde_json::Value::as_str)
                .is_some_and(|script| !script.trim().is_empty())
        };
        if root.join("package-lock.json").is_file() {
            commands.insert("setup".into(), command(&["npm", "ci"], 900));
        }
        if has_script("test") {
            commands.insert("check".into(), command(&["npm", "test"], 1800));
        }
        if has_script("build") {
            commands.insert("package".into(), command(&["npm", "run", "build"], 1800));
        }
    } else if root.join("pyproject.toml").is_file() {
        evidence.push("pyproject.toml canonical command inference".into());
        let metadata = fs::read_to_string(root.join("pyproject.toml")).unwrap_or_default();
        if metadata.contains("pytest") || root.join("tests").is_dir() {
            commands.insert("check".into(), command(&["python", "-m", "pytest"], 1800));
        }
    } else if root.join("go.mod").is_file() {
        evidence.push("go.mod canonical command inference".into());
        commands.insert("check".into(), command(&["go", "test", "./..."], 1800));
        commands.insert("package".into(), command(&["go", "build", "./..."], 1800));
    } else if root.join("main.tf").is_file() || root.join("terraform").is_dir() {
        evidence.push("Terraform canonical command inference".into());
        commands.insert(
            "setup".into(),
            command(&["terraform", "init", "-backend=false"], 900),
        );
        commands.insert(
            "format".into(),
            command(&["terraform", "fmt", "-check", "-recursive"], 300),
        );
        commands.insert("check".into(), command(&["terraform", "validate"], 900));
    }
    commands
}

fn command(argv: &[&str], timeout_seconds: u64) -> CommandSpec {
    CommandSpec {
        argv: argv.iter().map(ToString::to_string).collect(),
        working_directory: None,
        timeout_seconds: Some(timeout_seconds),
    }
}

fn discover_testing(
    commands: &BTreeMap<String, CommandSpec>,
    authorities: &BTreeMap<String, AuthorityRef>,
) -> Testing {
    let suites = ["format", "lint", "check"]
        .into_iter()
        .filter(|name| commands.contains_key(*name))
        .map(|name| TestSuite {
            id: name.into(),
            command: name.into(),
            stages: vec![TestStage::Local, TestStage::PreMerge, TestStage::PostMerge],
        })
        .collect();
    Testing {
        strategy_authority: authorities
            .contains_key("testing")
            .then(|| "testing".into()),
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
        suites,
    }
}

fn discover_context(authorities: &BTreeMap<String, AuthorityRef>) -> Context {
    let always = ["contracts"]
        .into_iter()
        .filter(|name| authorities.contains_key(*name))
        .map(ToString::to_string)
        .collect();
    let mut routes = BTreeMap::new();
    for (route, candidates) in [
        (
            "architecture_change",
            &["architecture", "decisions", "contracts"][..],
        ),
        (
            "code_change",
            &["implementation", "testing", "contracts"][..],
        ),
        ("delivery_change", &["delivery", "operations"][..]),
        ("effectiveness_evaluation", &["evaluation", "contracts"][..]),
    ] {
        let selected: Vec<_> = candidates
            .iter()
            .filter(|name| authorities.contains_key(**name))
            .map(ToString::to_string)
            .collect();
        if !selected.is_empty() {
            routes.insert(route.into(), selected);
        }
    }
    Context { always, routes }
}

fn default_quality_risks(kind: ProjectKind) -> Vec<QualityRisk> {
    let mut risks = vec![
        QualityRisk::Functional,
        QualityRisk::Reliability,
        QualityRisk::Security,
        QualityRisk::Compatibility,
        QualityRisk::Maintainability,
        QualityRisk::Recoverability,
    ];
    if matches!(
        kind,
        ProjectKind::Web | ProjectKind::Desktop | ProjectKind::Mobile
    ) {
        risks.push(QualityRisk::Usability);
        risks.push(QualityRisk::Accessibility);
    }
    risks
}

fn infer_delivery(kind: ProjectKind) -> Delivery {
    let (mode, artifact_kind, locator) = match kind {
        ProjectKind::Library => (DeliveryMode::Publish, "package", "registry:unconfigured"),
        ProjectKind::Cli => (DeliveryMode::Release, "binary", "release:unconfigured"),
        ProjectKind::Plugin => (DeliveryMode::Release, "plugin", "plugin:opdev"),
        ProjectKind::Documentation => (DeliveryMode::Render, "site", "site:unconfigured"),
        ProjectKind::Infrastructure | ProjectKind::Database => {
            (DeliveryMode::Apply, "change-set", "change-set:unconfigured")
        }
        ProjectKind::Firmware | ProjectKind::Embedded => {
            (DeliveryMode::Flash, "image", "image:unconfigured")
        }
        ProjectKind::Desktop | ProjectKind::Mobile => {
            (DeliveryMode::Publish, "application", "store:unconfigured")
        }
        ProjectKind::Service
        | ProjectKind::Web
        | ProjectKind::DataPipeline
        | ProjectKind::MachineLearning
        | ProjectKind::Generic => (DeliveryMode::Deploy, "artifact", "artifact:unconfigured"),
    };
    Delivery {
        status: DeliveryStatus::MigrationRequired,
        mode,
        artifact: Artifact {
            kind: artifact_kind.into(),
            locator: locator.into(),
        },
        environments: Vec::new(),
        recovery: Recovery {
            strategy: RecoveryStrategy::Unconfigured,
            command: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_urls_support_ssh_remotes() {
        assert_eq!(
            tracker_url("git@gitlab.com:group/project.git").as_deref(),
            Some("https://gitlab.com/group/project/-/issues")
        );
        assert_eq!(
            tracker_url("git@github.com:group/project.git").as_deref(),
            Some("https://github.com/group/project/issues")
        );
    }

    #[test]
    fn cargo_repository_infers_safe_commands() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        fs::create_dir(directory.path().join(".git"))?;
        fs::write(
            directory.path().join("Cargo.toml"),
            "[package]\nname='fixture'\n",
        )?;
        let discovery = discover(directory.path())?;
        assert_eq!(discovery.manifest.project.kind, ProjectKind::Library);
        assert!(discovery.manifest.commands.contains_key("check"));
        assert!(
            discovery
                .manifest
                .commands
                .values()
                .all(|command| !command.argv.join(" ").contains("sh -c"))
        );
        discovery.manifest.to_yaml()?;
        Ok(())
    }

    #[test]
    fn discovery_fixtures_cover_multiple_software_ecosystems()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixtures = [
            (
                "package.json",
                include_str!("../../../fixtures/discovery/node-web/package.json"),
                ProjectKind::Web,
                "check",
            ),
            (
                "pyproject.toml",
                include_str!("../../../fixtures/discovery/python-library/pyproject.toml"),
                ProjectKind::Library,
                "check",
            ),
            (
                "go.mod",
                include_str!("../../../fixtures/discovery/go-library/go.mod"),
                ProjectKind::Library,
                "package",
            ),
            (
                "main.tf",
                include_str!("../../../fixtures/discovery/terraform/main.tf"),
                ProjectKind::Infrastructure,
                "format",
            ),
        ];

        for (name, content, kind, expected_command) in fixtures {
            let directory = tempfile::tempdir()?;
            fs::create_dir(directory.path().join(".git"))?;
            fs::write(directory.path().join(name), content)?;
            if name == "package.json" {
                fs::write(directory.path().join("package-lock.json"), "{}")?;
            }
            let discovery = discover(directory.path())?;
            assert_eq!(discovery.manifest.project.kind, kind);
            assert!(discovery.manifest.commands.contains_key(expected_command));
            discovery.manifest.to_yaml()?;
        }
        Ok(())
    }

    #[test]
    fn node_discovery_does_not_invent_missing_scripts() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        fs::create_dir(directory.path().join(".git"))?;
        fs::write(
            directory.path().join("package.json"),
            r#"{"name":"no-scripts"}"#,
        )?;
        let discovery = discover(directory.path())?;
        assert!(!discovery.manifest.commands.contains_key("check"));
        assert!(!discovery.manifest.commands.contains_key("package"));
        Ok(())
    }
}
