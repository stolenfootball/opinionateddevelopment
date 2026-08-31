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
    /// Provider job image. Required by GitLab and unused by GitHub.
    pub job_image: Option<String>,
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
    /// No project-owned toolchain version could select a compatible GitLab image.
    #[error(
        "could not infer a GitLab toolchain image from {0}; add a supported toolchain version file or pass `--image`"
    )]
    ImageInference(PathBuf),
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
        let image = context.job_image.as_deref().ok_or_else(|| {
            CiError::InvalidTemplateValue("GitLab generation requires a job image".into())
        })?;
        if image.trim().is_empty() || image.contains(['\r', '\n']) {
            return Err(CiError::InvalidTemplateValue(
                "job image must be non-empty and single-line".into(),
            ));
        }
        let image = serde_json::to_string(image)
            .map_err(|error| CiError::InvalidTemplateValue(error.to_string()))?;
        Ok(render_template(GITLAB_TEMPLATE, context)?.replace("{{IMAGE_JSON}}", &image))
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

/// Infers a glibc-compatible official toolchain image from project-owned
/// version metadata. Mixed or custom stacks use the CLI's explicit `--image`
/// override rather than a silent guess.
///
/// # Errors
///
/// Returns [`CiError::ImageInference`] when no supported numeric toolchain or
/// language-family version is present.
pub fn infer_gitlab_image(root: &Path) -> Result<String, CiError> {
    if root.join("Cargo.toml").is_file()
        && let Some(version) = rust_version(root)
    {
        return Ok(format!("rust:{version}-trixie"));
    }
    if root.join("go.mod").is_file()
        && let Some(version) = go_version(root)
    {
        return Ok(format!("golang:{version}-trixie"));
    }
    if root.join("package.json").is_file()
        && let Some(version) = first_version_file(root, &[".nvmrc", ".node-version"])
    {
        return Ok(format!("node:{version}-trixie"));
    }
    if root.join("pyproject.toml").is_file()
        && let Some(version) = first_version_file(root, &[".python-version"])
    {
        return Ok(format!("python:{version}-trixie"));
    }
    Err(CiError::ImageInference(root.to_path_buf()))
}

fn rust_version(root: &Path) -> Option<String> {
    let toml = fs::read_to_string(root.join("rust-toolchain.toml")).ok();
    if let Some(version) = toml.as_deref().and_then(|source| {
        source.lines().find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name.trim() == "channel").then(|| value.trim().trim_matches(['\'', '"']))
        })
    }) && valid_toolchain_version(version)
    {
        return Some(version.to_owned());
    }
    first_version_file(root, &["rust-toolchain"])
}

fn go_version(root: &Path) -> Option<String> {
    let source = fs::read_to_string(root.join("go.mod")).ok()?;
    let mut language_version = None;
    for line in source.lines() {
        let mut fields = line.split_whitespace();
        match (fields.next(), fields.next()) {
            (Some("toolchain"), Some(version)) => {
                let version = version.strip_prefix("go").unwrap_or(version);
                if let Some(family) = go_language_family(version) {
                    return Some(family);
                }
            }
            (Some("go"), Some(version)) => {
                language_version = go_language_family(version);
            }
            _ => {}
        }
    }
    language_version
}

fn go_language_family(version: &str) -> Option<String> {
    let mut segments = version.split('.');
    let major = segments.next()?;
    let minor = segments.next()?;
    if ![major, minor].iter().all(|segment| {
        !segment.is_empty() && segment.chars().all(|character| character.is_ascii_digit())
    }) || !segments.all(|segment| {
        !segment.is_empty() && segment.chars().all(|character| character.is_ascii_digit())
    }) {
        return None;
    }
    Some(format!("{major}.{minor}"))
}

fn first_version_file(root: &Path, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        let version = fs::read_to_string(root.join(name)).ok()?;
        let version = version.trim().trim_start_matches('v');
        valid_toolchain_version(version).then(|| version.to_owned())
    })
}

fn valid_toolchain_version(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|segment| {
            !segment.is_empty() && segment.chars().all(|character| character.is_ascii_digit())
        })
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

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    fn context() -> TemplateContext {
        TemplateContext {
            opdev_version: "0.1.0".into(),
            trunk: "main".into(),
            job_image: Some("rust:1.97.0-trixie".into()),
        }
    }

    fn rendered(provider: CiProvider) -> Result<String, Box<dyn std::error::Error>> {
        Ok(adapter_for(provider)?.render(&context())?)
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
    fn generated_installers_use_versioned_archives_outside_the_worktree()
    -> Result<(), Box<dyn std::error::Error>> {
        let github = rendered(CiProvider::Github)?;
        assert!(
            github.contains("archive=\"opdev-${OPDEV_VERSION}-x86_64-unknown-linux-gnu.tar.gz\"")
        );
        assert!(github.contains("mktemp -d \"$RUNNER_TEMP/opdev-install.XXXXXX\""));
        assert!(github.contains("--output \"$install_dir/$archive\""));
        assert!(!github.contains("--output \"$archive\""));

        let gitlab = rendered(CiProvider::Gitlab)?;
        assert!(
            gitlab.contains("archive=\"opdev-${OPDEV_VERSION}-x86_64-unknown-linux-gnu.tar.gz\"")
        );
        assert!(gitlab.contains("image: \"rust:1.97.0-trixie\""));
        assert!(gitlab.contains("install_dir=\"$(mktemp -d)\""));
        assert!(gitlab.contains("--output \"$install_dir/$archive\""));
        assert!(!gitlab.contains("--output \"$archive\""));
        Ok(())
    }

    #[test]
    fn gitlab_images_are_inferred_from_project_version_metadata()
    -> Result<(), Box<dyn std::error::Error>> {
        let rust = tempfile::tempdir()?;
        fs::write(
            rust.path().join("Cargo.toml"),
            "[package]\nname = \"fixture\"\n",
        )?;
        fs::write(
            rust.path().join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.97.0\"\n",
        )?;
        assert_eq!(infer_gitlab_image(rust.path())?, "rust:1.97.0-trixie");

        let go = tempfile::tempdir()?;
        fs::write(
            go.path().join("go.mod"),
            "module example.test/fixture\n\ngo 1.25\ntoolchain go1.25.1\n",
        )?;
        assert_eq!(infer_gitlab_image(go.path())?, "golang:1.25-trixie");

        let go_minimum = tempfile::tempdir()?;
        fs::write(
            go_minimum.path().join("go.mod"),
            "module example.test/fixture\n\ngo 1.24.0\n",
        )?;
        assert_eq!(infer_gitlab_image(go_minimum.path())?, "golang:1.24-trixie");

        let node = tempfile::tempdir()?;
        fs::write(node.path().join("package.json"), "{}\n")?;
        fs::write(node.path().join(".nvmrc"), "v24.4.1\n")?;
        assert_eq!(infer_gitlab_image(node.path())?, "node:24.4.1-trixie");

        let python = tempfile::tempdir()?;
        fs::write(
            python.path().join("pyproject.toml"),
            "[project]\nname = \"fixture\"\n",
        )?;
        fs::write(python.path().join(".python-version"), "3.14.0\n")?;
        assert_eq!(infer_gitlab_image(python.path())?, "python:3.14.0-trixie");
        Ok(())
    }

    #[test]
    fn gitlab_image_inference_refuses_to_guess() -> Result<(), Box<dyn std::error::Error>> {
        let project = tempfile::tempdir()?;
        fs::write(project.path().join("package.json"), "{}\n")?;
        assert!(matches!(
            infer_gitlab_image(project.path()),
            Err(CiError::ImageInference(_))
        ));
        Ok(())
    }

    #[test]
    #[ignore = "requires Docker and network access to the published OpDev release"]
    fn rendered_gitlab_installer_runs_with_rust_and_go_toolchains()
    -> Result<(), Box<dyn std::error::Error>> {
        for (image, toolchain_check) in [
            (
                "rust:1.97.0-trixie",
                "fixture=$(mktemp -d) && cd \"$fixture\" && mkdir src && printf '[package]\\nname = \"fixture\"\\nversion = \"0.1.0\"\\nedition = \"2024\"\\n' > Cargo.toml && printf '#[test]\\nfn passes() { assert_eq!(2 + 2, 4); }\\n' > src/lib.rs && cargo test",
            ),
            (
                "golang:1.24-trixie",
                "fixture=$(mktemp -d) && cd \"$fixture\" && go mod init example.test/fixture && printf 'package fixture\\n\\nimport \"testing\"\\n\\nfunc TestPasses(t *testing.T) {}\\n' > fixture_test.go && go test ./...",
            ),
        ] {
            let mut context = context();
            context.job_image = Some(image.into());
            let rendered = adapter_for(CiProvider::Gitlab)?.render(&context)?;
            let parsed: serde_json::Value = serde_saphyr::from_str(&rendered)?;
            let mut script = parsed["opdev"]["before_script"]
                .as_array()
                .ok_or_else(|| std::io::Error::other("before_script is not an array"))?
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| std::io::Error::other("before_script entry is not a string"))
                })
                .collect::<Result<Vec<_>, _>>()?
                .join("\n");
            script.push('\n');
            script.push_str(toolchain_check);

            let output = Command::new("docker")
                .args([
                    "run",
                    "--rm",
                    "--env",
                    "OPDEV_VERSION=0.1.0",
                    image,
                    "sh",
                    "-c",
                    &script,
                ])
                .output()?;
            assert!(
                output.status.success(),
                "rendered GitLab installer failed in {image}: stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn rendered_installers_execute_a_release_fixture_without_dirtying_git()
    -> Result<(), Box<dyn std::error::Error>> {
        for provider in [CiProvider::Github, CiProvider::Gitlab] {
            execute_installer_fixture(provider)?;
        }
        Ok(())
    }

    #[test]
    fn missing_configuration_is_a_migration_not_a_pass() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let inspection = adapter_for(CiProvider::Github)?.inspect(directory.path())?;
        assert_eq!(inspection.configuration.outcome, Outcome::MigrationRequired);
        Ok(())
    }

    #[cfg(unix)]
    fn execute_installer_fixture(provider: CiProvider) -> Result<(), Box<dyn std::error::Error>> {
        let project = tempfile::tempdir()?;
        run(Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(project.path()))?;
        fs::write(project.path().join("README.md"), "fixture\n")?;
        let adapter = adapter_for(provider)?;
        write_new(adapter, project.path(), &context())?;
        run(Command::new("git")
            .args(["add", "--all"])
            .current_dir(project.path()))?;
        let fingerprint = opdev_project::staged_fingerprint(project.path())?;

        let fixture = tempfile::tempdir()?;
        let payload = fixture.path().join("payload");
        fs::create_dir(&payload)?;
        let executable = payload.join("opdev");
        fs::write(&executable, "#!/bin/sh\nexit 0\n")?;
        let mut permissions = fs::metadata(&executable)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions)?;
        let archive = "opdev-0.1.0-x86_64-unknown-linux-gnu.tar.gz";
        run(Command::new("tar")
            .args(["-czf", archive, "-C", "payload", "opdev"])
            .current_dir(fixture.path()))?;
        let checksum = Command::new("sha256sum")
            .arg(archive)
            .current_dir(fixture.path())
            .output()?;
        if !checksum.status.success() {
            return Err(std::io::Error::other("sha256sum fixture generation failed").into());
        }
        fs::write(fixture.path().join("SHA256SUMS"), checksum.stdout)?;

        let fake_bin = fixture.path().join("bin");
        fs::create_dir(&fake_bin)?;
        let fake_curl = fake_bin.join("curl");
        fs::write(
            &fake_curl,
            format!(
                "#!/bin/sh\nset -eu\noutput=\nurl=\nwhile [ \"$#\" -gt 0 ]; do\n  case \"$1\" in\n    --output) output=$2; shift 2 ;;\n    *) url=$1; shift ;;\n  esac\ndone\ncase \"$url\" in\n  */SHA256SUMS) cp \"$FIXTURE_RELEASE/SHA256SUMS\" \"$output\" ;;\n  *) cp \"$FIXTURE_RELEASE/{archive}\" \"$output\" ;;\nesac\n"
            ),
        )?;
        let mut permissions = fs::metadata(&fake_curl)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_curl, permissions)?;

        let rendered = rendered(provider)?;
        let parsed: serde_json::Value = serde_saphyr::from_str(&rendered)?;
        let mut script = match provider {
            CiProvider::Github => parsed["jobs"]["opdev"]["steps"][1]["run"]
                .as_str()
                .ok_or_else(|| std::io::Error::other("GitHub install step is not a string"))?
                .to_owned(),
            CiProvider::Gitlab => parsed["opdev"]["before_script"]
                .as_array()
                .ok_or_else(|| std::io::Error::other("GitLab before_script is not an array"))?
                .iter()
                .filter_map(serde_json::Value::as_str)
                .filter(|command| !command.starts_with("apk add"))
                .collect::<Vec<_>>()
                .join("\n"),
            CiProvider::Other | CiProvider::Unconfigured => {
                return Err(std::io::Error::other("unsupported fixture provider").into());
            }
        };
        if provider == CiProvider::Gitlab {
            script.push_str("\nopdev version\n");
        }
        let runner_temp = tempfile::tempdir()?;
        let path = format!(
            "{}:{}",
            fake_bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        run(Command::new("bash")
            .arg("-c")
            .arg(script)
            .env("FIXTURE_RELEASE", fixture.path())
            .env("OPDEV_VERSION", "0.1.0")
            .env("RUNNER_TEMP", runner_temp.path())
            .env("PATH", path)
            .current_dir(project.path()))?;
        assert_eq!(
            opdev_project::staged_fingerprint(project.path())?,
            fingerprint
        );
        Ok(())
    }

    #[cfg(unix)]
    fn run(command: &mut Command) -> Result<(), Box<dyn std::error::Error>> {
        let output = command.output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "command failed: stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ))
            .into())
        }
    }
}
