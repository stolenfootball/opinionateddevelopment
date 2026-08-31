//! Read-only remote repository and trunk-pipeline auditing.

#![forbid(unsafe_code)]

use std::env;
use std::process::{Command, Stdio};
use std::time::Duration;

use opdev_core::{Evidence, Outcome};
use opdev_project::{CiProvider, ProjectManifest};
use reqwest::blocking::{Client, RequestBuilder};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

/// One remote capability and the evidence supporting it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCapability {
    /// Exhaustive audit outcome.
    pub outcome: Outcome,
    /// Read-only facts returned by the provider.
    pub evidence: Vec<Evidence>,
    /// Permission, absence, mismatch, or provider diagnostic.
    pub diagnostic: Option<String>,
}

/// Provider-neutral remote audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteAudit {
    /// Repository identity used for API requests.
    pub repository: String,
    /// The provider has a pipeline on the declared trunk.
    pub ci: RemoteCapability,
    /// Provider default branch matches the declared trunk.
    pub trunk: RemoteCapability,
    /// The declared trunk has provider branch protection.
    pub trunk_protection: RemoteCapability,
    /// Latest trunk-pipeline verdict.
    pub trunk_pipeline: RemoteCapability,
    /// Automatic merged-branch cleanup setting.
    pub branch_lifecycle: RemoteCapability,
}

/// Failures that prevent a remote audit from being attempted safely.
#[derive(Debug, Error)]
pub enum RemoteError {
    /// The initialized project has no remote URL.
    #[error("the project contract does not declare a remote repository")]
    MissingRemote,
    /// The remote is not a supported GitHub or GitLab repository URL.
    #[error("unsupported or malformed remote repository `{0}`")]
    InvalidRemote(String),
    /// Manifest and remote provider disagree.
    #[error("project declares {declared:?} CI but the remote belongs to {detected:?}")]
    ProviderMismatch {
        /// Provider selected by the manifest.
        declared: CiProvider,
        /// Provider detected from the remote host.
        detected: CiProvider,
    },
    /// The HTTP client could not be constructed.
    #[error("could not initialize the read-only remote client: {0}")]
    Client(reqwest::Error),
}

/// Audits first-class remote settings and latest trunk status using GET-only
/// provider API requests.
///
/// Authentication is optional. GitHub reads `OPDEV_GITHUB_TOKEN`,
/// `GITHUB_TOKEN`, then `GH_TOKEN`. GitLab uses the credential precedence
/// documented in `spec/remote-audits.md`, including an authenticated `glab`
/// fallback. Tokens are never included in evidence or diagnostics.
///
/// # Errors
///
/// Returns [`RemoteError`] before network access when the remote is missing,
/// malformed, unsupported, or inconsistent with the declared provider, or when
/// the read-only HTTP client cannot be created. HTTP and authorization failures
/// become `unverified` capabilities in the returned audit.
pub fn audit(manifest: &ProjectManifest) -> Result<RemoteAudit, RemoteError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let remote = manifest
        .project
        .ci
        .remote
        .as_deref()
        .ok_or(RemoteError::MissingRemote)?;
    let repository = Repository::parse(remote)?;
    if manifest.project.ci.provider != repository.provider {
        return Err(RemoteError::ProviderMismatch {
            declared: manifest.project.ci.provider,
            detected: repository.provider,
        });
    }
    let client = Client::builder()
        .user_agent(concat!("opdev/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(RemoteError::Client)?;
    match repository.provider {
        CiProvider::Github => Ok(audit_github(&client, &repository, &manifest.project.trunk)),
        CiProvider::Gitlab => Ok(audit_gitlab(&client, &repository, &manifest.project.trunk)),
        CiProvider::Other | CiProvider::Unconfigured => {
            Err(RemoteError::InvalidRemote(remote.into()))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Repository {
    provider: CiProvider,
    namespace: String,
    name: String,
}

impl Repository {
    fn parse(remote: &str) -> Result<Self, RemoteError> {
        if let Some(path) = remote.strip_prefix("git@github.com:") {
            return Self::from_parts(CiProvider::Github, path, remote);
        }
        if let Some(path) = remote.strip_prefix("git@gitlab.com:") {
            return Self::from_parts(CiProvider::Gitlab, path, remote);
        }
        let parsed =
            url::Url::parse(remote).map_err(|_| RemoteError::InvalidRemote(remote.to_owned()))?;
        let provider = match parsed.host_str() {
            Some("github.com") => CiProvider::Github,
            Some("gitlab.com") => CiProvider::Gitlab,
            _ => return Err(RemoteError::InvalidRemote(remote.to_owned())),
        };
        Self::from_parts(provider, parsed.path().trim_start_matches('/'), remote)
    }

    fn from_parts(provider: CiProvider, path: &str, original: &str) -> Result<Self, RemoteError> {
        let path = path.trim_end_matches('/').trim_end_matches(".git");
        let Some((namespace, name)) = path.rsplit_once('/') else {
            return Err(RemoteError::InvalidRemote(original.to_owned()));
        };
        if namespace.is_empty() || name.is_empty() {
            return Err(RemoteError::InvalidRemote(original.to_owned()));
        }
        Ok(Self {
            provider,
            namespace: namespace.into(),
            name: name.into(),
        })
    }

    fn slug(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }
}

#[derive(Debug, Deserialize)]
struct GithubRepository {
    default_branch: String,
    delete_branch_on_merge: bool,
}

#[derive(Debug, Deserialize)]
struct GithubRuns {
    total_count: u64,
    workflow_runs: Vec<GithubRun>,
}

#[derive(Debug, Deserialize)]
struct GithubRun {
    status: String,
    conclusion: Option<String>,
    html_url: String,
}

fn audit_github(client: &Client, repository: &Repository, trunk: &str) -> RemoteAudit {
    let slug = repository.slug();
    let base = format!("https://api.github.com/repos/{slug}");
    let token = first_env(&["OPDEV_GITHUB_TOKEN", "GITHUB_TOKEN", "GH_TOKEN"]);
    let project =
        get_json::<GithubRepository>(github_request(client.get(&base), token.as_deref()), &base);
    let Ok(project) = project else {
        return unverified_audit(&slug, project.err().as_deref());
    };
    let trunk_result = compare_trunk(&slug, &project.default_branch, trunk);
    let protection_url = format!("{base}/branches/{}/protection", encode(trunk));
    let protection = get_status(
        github_request(client.get(&protection_url), token.as_deref()),
        &protection_url,
    );
    let runs_url = format!("{base}/actions/runs?branch={}&per_page=1", encode(trunk));
    let runs = get_json::<GithubRuns>(
        github_request(client.get(&runs_url), token.as_deref()),
        &runs_url,
    );
    let (ci, pipeline) = match runs {
        Ok(runs) if runs.total_count > 0 => {
            if let Some(run) = runs.workflow_runs.first() {
                (
                    passed(
                        "remote_ci",
                        "A trunk workflow run exists",
                        Some(&run.html_url),
                    ),
                    pipeline_capability(&run.status, run.conclusion.as_deref(), &run.html_url),
                )
            } else {
                let missing = migration("The provider reports no trunk workflow runs");
                (missing.clone(), missing)
            }
        }
        Ok(_) => {
            let missing = migration("The provider reports no trunk workflow runs");
            (missing.clone(), missing)
        }
        Err(diagnostic) => {
            let unavailable = unverified(&diagnostic);
            (unavailable.clone(), unavailable)
        }
    };
    RemoteAudit {
        repository: slug,
        ci,
        trunk: trunk_result,
        trunk_protection: protection_capability(protection, &protection_url),
        trunk_pipeline: pipeline,
        branch_lifecycle: if project.delete_branch_on_merge {
            passed(
                "remote_policy",
                "Merged branches are configured for automatic deletion",
                Some(&base),
            )
        } else {
            failed("Merged branches are not configured for automatic deletion")
        },
    }
}

#[derive(Debug, Deserialize)]
struct GitlabProject {
    id: u64,
    default_branch: Option<String>,
    remove_source_branch_after_merge: Option<bool>,
    web_url: String,
}

#[derive(Debug, Deserialize)]
struct GitlabPipeline {
    status: String,
    web_url: String,
}

#[derive(Debug, Deserialize)]
struct GitlabProtectedBranch {
    allow_force_push: bool,
}

fn audit_gitlab(client: &Client, repository: &Repository, trunk: &str) -> RemoteAudit {
    let slug = repository.slug();
    let encoded_slug = encode(&slug);
    let base = format!("https://gitlab.com/api/v4/projects/{encoded_slug}");
    let credential = gitlab_credential();
    let project = get_json::<GitlabProject>(
        gitlab_request(client.get(&base), credential.as_ref()),
        &base,
    );
    let Ok(project) = project else {
        return unverified_audit(&slug, project.err().as_deref());
    };
    let trunk_result = project.default_branch.as_deref().map_or_else(
        || unverified("The provider did not return a default branch"),
        |default_branch| compare_trunk(&slug, default_branch, trunk),
    );
    let protection_url = format!(
        "https://gitlab.com/api/v4/projects/{}/protected_branches/{}",
        project.id,
        encode(trunk)
    );
    let protection = get_json::<GitlabProtectedBranch>(
        gitlab_request(client.get(&protection_url), credential.as_ref()),
        &protection_url,
    );
    let pipeline_url = format!(
        "https://gitlab.com/api/v4/projects/{}/pipelines?ref={}&per_page=1",
        project.id,
        encode(trunk)
    );
    let pipelines = get_json::<Vec<GitlabPipeline>>(
        gitlab_request(client.get(&pipeline_url), credential.as_ref()),
        &pipeline_url,
    );
    let (ci, pipeline) = match pipelines {
        Ok(pipelines) => pipelines.first().map_or_else(
            || {
                let missing = migration("The provider reports no trunk pipelines");
                (missing.clone(), missing)
            },
            |pipeline| {
                (
                    passed(
                        "remote_ci",
                        "A trunk pipeline exists",
                        Some(&pipeline.web_url),
                    ),
                    pipeline_capability(
                        &pipeline.status,
                        Some(&pipeline.status),
                        &pipeline.web_url,
                    ),
                )
            },
        ),
        Err(diagnostic) => {
            let unavailable = unverified(&diagnostic);
            (unavailable.clone(), unavailable)
        }
    };
    let protection = match protection {
        Ok(protection) if !protection.allow_force_push => passed(
            "remote_policy",
            "The declared trunk is protected and force-push is disabled",
            Some(&protection_url),
        ),
        Ok(_) => failed("The declared trunk permits force-push"),
        Err(diagnostic) => unverified(&diagnostic),
    };
    RemoteAudit {
        repository: slug,
        ci,
        trunk: trunk_result,
        trunk_protection: protection,
        trunk_pipeline: pipeline,
        branch_lifecycle: match project.remove_source_branch_after_merge {
            Some(true) => passed(
                "remote_policy",
                "Merged branches are configured for automatic deletion",
                Some(&project.web_url),
            ),
            Some(false) => failed("Merged branches are not configured for automatic deletion"),
            None => {
                unverified("The provider did not expose merged-branch cleanup to this credential")
            }
        },
    }
}

fn github_request(request: RequestBuilder, token: Option<&str>) -> RequestBuilder {
    if let Some(token) = token {
        request.bearer_auth(token)
    } else {
        request
    }
}

fn gitlab_request(
    request: RequestBuilder,
    credential: Option<&GitlabCredential>,
) -> RequestBuilder {
    match credential {
        Some(credential) if credential.scheme == GitlabAuthScheme::Bearer => {
            request.bearer_auth(&credential.secret)
        }
        Some(credential) => request.header("PRIVATE-TOKEN", &credential.secret),
        None => request,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GitlabAuthScheme {
    Bearer,
    PrivateToken,
}

struct GitlabCredential {
    scheme: GitlabAuthScheme,
    secret: String,
}

fn gitlab_credential() -> Option<GitlabCredential> {
    gitlab_credential_with(|name| env::var(name).ok(), glab_token)
}

fn gitlab_credential_with<F, G>(mut environment: F, glab: G) -> Option<GitlabCredential>
where
    F: FnMut(&str) -> Option<String>,
    G: FnOnce() -> Option<String>,
{
    for (name, scheme) in [
        ("OPDEV_GITLAB_OAUTH_TOKEN", GitlabAuthScheme::Bearer),
        ("OPDEV_GITLAB_PRIVATE_TOKEN", GitlabAuthScheme::PrivateToken),
        ("OPDEV_GITLAB_TOKEN", GitlabAuthScheme::Bearer),
        ("GITLAB_TOKEN", GitlabAuthScheme::Bearer),
        ("GLAB_TOKEN", GitlabAuthScheme::Bearer),
    ] {
        if let Some(secret) = environment(name).filter(|value| !value.trim().is_empty()) {
            return Some(GitlabCredential { scheme, secret });
        }
    }
    glab().map(|secret| GitlabCredential {
        scheme: GitlabAuthScheme::Bearer,
        secret,
    })
}

fn glab_token() -> Option<String> {
    let output = Command::new("glab")
        .args(["config", "get", "token", "--host", "gitlab.com"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8(output.stdout).ok()?;
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_owned())
}

fn get_json<T: DeserializeOwned>(request: RequestBuilder, location: &str) -> Result<T, String> {
    let response = request
        .send()
        .map_err(|error| format!("GET {location} could not complete: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "GET {location} returned HTTP {}; authentication or permission may be required",
            response.status()
        ));
    }
    response
        .json()
        .map_err(|error| format!("GET {location} returned invalid JSON: {error}"))
}

fn get_status(request: RequestBuilder, location: &str) -> Result<(), String> {
    let response = request
        .send()
        .map_err(|error| format!("GET {location} could not complete: {error}"))?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "GET {location} returned HTTP {}",
            response.status()
        ))
    }
}

fn compare_trunk(repository: &str, provider_trunk: &str, declared_trunk: &str) -> RemoteCapability {
    if provider_trunk == declared_trunk {
        passed(
            "remote_policy",
            &format!("Provider default branch is `{declared_trunk}`"),
            Some(repository),
        )
    } else {
        failed(&format!(
            "Provider default branch `{provider_trunk}` differs from declared trunk `{declared_trunk}`"
        ))
    }
}

fn protection_capability(result: Result<(), String>, location: &str) -> RemoteCapability {
    match result {
        Ok(()) => passed(
            "remote_policy",
            "The declared trunk has branch protection",
            Some(location),
        ),
        Err(diagnostic) => unverified(&diagnostic),
    }
}

fn pipeline_capability(status: &str, conclusion: Option<&str>, location: &str) -> RemoteCapability {
    let verdict = conclusion.unwrap_or(status);
    match verdict {
        "success" | "successful" => passed(
            "remote_pipeline",
            "The latest trunk pipeline succeeded",
            Some(location),
        ),
        "failure" | "failed" | "cancelled" | "canceled" | "timed_out" => {
            failed(&format!("The latest trunk pipeline concluded `{verdict}`"))
        }
        _ => unverified(&format!(
            "The latest trunk pipeline is not definitive (`{status}` / `{verdict}`)"
        )),
    }
}

fn passed(kind: &str, summary: &str, location: Option<&str>) -> RemoteCapability {
    RemoteCapability {
        outcome: Outcome::Passed,
        evidence: vec![Evidence {
            kind: kind.into(),
            summary: summary.into(),
            location: location.map(ToOwned::to_owned),
        }],
        diagnostic: None,
    }
}

fn failed(diagnostic: &str) -> RemoteCapability {
    RemoteCapability {
        outcome: Outcome::Failed,
        evidence: Vec::new(),
        diagnostic: Some(diagnostic.into()),
    }
}

fn unverified(diagnostic: &str) -> RemoteCapability {
    RemoteCapability {
        outcome: Outcome::Unverified,
        evidence: Vec::new(),
        diagnostic: Some(diagnostic.into()),
    }
}

fn migration(diagnostic: &str) -> RemoteCapability {
    RemoteCapability {
        outcome: Outcome::MigrationRequired,
        evidence: Vec::new(),
        diagnostic: Some(diagnostic.into()),
    }
}

fn unverified_audit(repository: &str, diagnostic: Option<&str>) -> RemoteAudit {
    let capability = unverified(diagnostic.unwrap_or("Remote repository evidence is unavailable"));
    RemoteAudit {
        repository: repository.into(),
        ci: capability.clone(),
        trunk: capability.clone(),
        trunk_protection: capability.clone(),
        trunk_pipeline: capability.clone(),
        branch_lifecycle: capability,
    }
}

fn first_env(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| env::var(name).ok().filter(|value| !value.trim().is_empty()))
}

fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::{self, Receiver};
    use std::thread::JoinHandle;

    type MockServer = (String, Receiver<String>, JoinHandle<Result<(), String>>);

    #[test]
    fn parses_first_class_https_and_ssh_remotes() -> Result<(), Box<dyn std::error::Error>> {
        for (remote, provider, slug) in [
            (
                "git@github.com:group/project.git",
                CiProvider::Github,
                "group/project",
            ),
            (
                "https://gitlab.com/group/subgroup/project.git",
                CiProvider::Gitlab,
                "group/subgroup/project",
            ),
            (
                "ssh://git@github.com/group/project.git",
                CiProvider::Github,
                "group/project",
            ),
        ] {
            let parsed = Repository::parse(remote)?;
            assert_eq!(parsed.provider, provider);
            assert_eq!(parsed.slug(), slug);
        }
        Ok(())
    }

    #[test]
    fn rejects_unknown_hosts_and_ambiguous_paths() {
        assert!(Repository::parse("https://example.com/group/project").is_err());
        assert!(Repository::parse("git@github.com:project.git").is_err());
    }

    #[test]
    fn pipeline_states_never_turn_unknown_into_passed() {
        assert_eq!(
            pipeline_capability("completed", Some("success"), "run").outcome,
            Outcome::Passed
        );
        assert_eq!(
            pipeline_capability("in_progress", None, "run").outcome,
            Outcome::Unverified
        );
        assert_eq!(
            pipeline_capability("completed", Some("failure"), "run").outcome,
            Outcome::Failed
        );
    }

    #[test]
    fn gitlab_project_ids_encode_nested_namespaces() {
        assert_eq!(encode("group/sub/project"), "group%2Fsub%2Fproject");
    }

    #[test]
    fn gitlab_credential_precedence_is_deterministic() -> Result<(), Box<dyn std::error::Error>> {
        let values = BTreeMap::from([
            ("OPDEV_GITLAB_OAUTH_TOKEN", "oauth"),
            ("OPDEV_GITLAB_PRIVATE_TOKEN", "private"),
            ("OPDEV_GITLAB_TOKEN", "generic"),
            ("GITLAB_TOKEN", "gitlab"),
            ("GLAB_TOKEN", "glab-env"),
        ]);
        let selected = gitlab_credential_with(
            |name| values.get(name).map(ToString::to_string),
            || Some("glab-store".into()),
        )
        .ok_or("a credential should be selected")?;
        assert!(selected.scheme == GitlabAuthScheme::Bearer);
        assert_eq!(selected.secret, "oauth");

        let selected = gitlab_credential_with(
            |name| (name == "OPDEV_GITLAB_PRIVATE_TOKEN").then(|| "private".into()),
            || Some("glab-store".into()),
        )
        .ok_or("the private credential should be selected")?;
        assert!(selected.scheme == GitlabAuthScheme::PrivateToken);

        let selected = gitlab_credential_with(|_| None, || Some("glab-store".into()))
            .ok_or("the glab fallback should be selected")?;
        assert!(selected.scheme == GitlabAuthScheme::Bearer);
        assert_eq!(selected.secret, "glab-store");
        Ok(())
    }

    #[test]
    fn gitlab_mock_accepts_bearer_and_private_token_headers()
    -> Result<(), Box<dyn std::error::Error>> {
        let client = test_client()?;
        for (scheme, expected, absent) in [
            (
                GitlabAuthScheme::Bearer,
                "authorization: Bearer bearer-fixture",
                "private-token:",
            ),
            (
                GitlabAuthScheme::PrivateToken,
                "private-token: private-fixture",
                "authorization:",
            ),
        ] {
            let secret = match scheme {
                GitlabAuthScheme::Bearer => "bearer-fixture",
                GitlabAuthScheme::PrivateToken => "private-fixture",
            };
            let (url, request, server) = mock_response(200, "{}")?;
            let credential = GitlabCredential {
                scheme,
                secret: secret.into(),
            };
            let _: BTreeMap<String, String> =
                get_json(gitlab_request(client.get(&url), Some(&credential)), &url)?;
            let request = request.recv()?;
            server.join().map_err(|_| "mock server panicked")??;
            let request_lower = request.to_ascii_lowercase();
            assert!(request_lower.contains(&expected.to_ascii_lowercase()));
            assert!(!request_lower.contains(absent));
        }
        Ok(())
    }

    #[test]
    fn gitlab_unauthorized_response_fails_closed_without_disclosing_token()
    -> Result<(), Box<dyn std::error::Error>> {
        let client = test_client()?;
        let (url, request, server) = mock_response(401, r#"{"message":"401 Unauthorized"}"#)?;
        let credential = GitlabCredential {
            scheme: GitlabAuthScheme::Bearer,
            secret: "never-log-this-fixture".into(),
        };
        let Err(error) = get_json::<BTreeMap<String, String>>(
            gitlab_request(client.get(&url), Some(&credential)),
            &url,
        ) else {
            return Err("401 unexpectedly produced remote evidence".into());
        };
        let request = request.recv()?;
        server.join().map_err(|_| "mock server panicked")??;
        assert!(request.contains("never-log-this-fixture"));
        assert!(error.contains("HTTP 401"));
        assert!(!error.contains("never-log-this-fixture"));
        Ok(())
    }

    #[test]
    #[ignore = "requires OPDEV_TEST_GITLAB_PRIVATE_REMOTE and an authenticated glab session"]
    fn live_private_gitlab_audit_uses_glab_oauth() -> Result<(), Box<dyn std::error::Error>> {
        let remote = std::env::var("OPDEV_TEST_GITLAB_PRIVATE_REMOTE")?;
        let manifest_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.opdev/project.yaml");
        let mut manifest = opdev_project::ProjectManifest::load(&manifest_path)?;
        manifest.project.ci.provider = CiProvider::Gitlab;
        manifest.project.ci.remote = Some(remote);
        manifest.project.trunk = "main".into();

        let result = audit(&manifest)?;
        assert_eq!(result.trunk.outcome, Outcome::Passed);
        assert!(!result.repository.is_empty());
        Ok(())
    }

    fn mock_response(status: u16, body: &'static str) -> Result<MockServer, std::io::Error> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let (sender, receiver) = mpsc::channel();
        let server = std::thread::spawn(move || -> Result<(), String> {
            let (mut stream, _) = listener
                .accept()
                .map_err(|error| format!("mock request could not connect: {error}"))?;
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let read = stream
                    .read(&mut buffer)
                    .map_err(|error| format!("mock request could not read: {error}"))?;
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            sender
                .send(String::from_utf8_lossy(&request).into_owned())
                .map_err(|error| format!("test could not receive mock request: {error}"))?;
            let reason = if status == 200 { "OK" } else { "Unauthorized" };
            write!(
                stream,
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .map_err(|error| format!("mock response could not write: {error}"))?;
            Ok(())
        });
        Ok((format!("http://{address}/api/v4/project"), receiver, server))
    }

    fn test_client() -> Result<Client, reqwest::Error> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        Client::builder().build()
    }
}
