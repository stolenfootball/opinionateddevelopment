# Read-only remote audits

`opdev check --remote` and `opdev doctor --remote` query GitHub or GitLab with GET requests only. OpDev does not create pipelines, change branch protection, modify merge settings, delete branches, post statuses, or otherwise mutate a remote repository. Local CI generation remains a separate explicit operation.

Remote URLs are accepted only for the first-class `github.com` and `gitlab.com` hosts in 0.1. This prevents a repository-controlled remote from turning the audit into a request to an arbitrary host. Self-managed instances and additional providers require a future adapter with an explicit trusted API origin.

Authentication is optional. GitHub reads `OPDEV_GITHUB_TOKEN`, `GITHUB_TOKEN`,
then `GH_TOKEN`, and sends the selected value as a bearer credential. GitLab
uses the first non-empty source in this exact order:

1. `OPDEV_GITLAB_OAUTH_TOKEN`, sent as `Authorization: Bearer`;
2. `OPDEV_GITLAB_PRIVATE_TOKEN`, sent as `PRIVATE-TOKEN` for explicit
   compatibility;
3. `OPDEV_GITLAB_TOKEN`, `GITLAB_TOKEN`, then `GLAB_TOKEN`, each sent as
   `Authorization: Bearer`; and
4. the `gitlab.com` token returned by an authenticated
   `glab config get token --host gitlab.com`, also sent as Bearer.

GitLab documents Bearer authentication for OAuth tokens and for personal,
project, and group access tokens, so generic credentials do not need secret
shape detection. OpDev does not retry a rejected credential under another
header. The optional `glab` lookup keeps browser or device OAuth login seamless
without reading credential files or keyrings directly. A missing `glab`
executable or credential simply leaves the audit unauthenticated.

Tokens are added only to request headers, captured in memory only as long as
needed, and never copied into evidence or diagnostics. Missing permission,
unavailable fields, HTTP failures, and non-definitive pipeline states produce
`unverified`; they do not become passes. In particular, HTTP 401 never falls
back to public evidence. A missing trunk pipeline is `migration_required`,
while a definitive failing trunk pipeline is `failed`.

The audit verifies the provider default branch, trunk protection visibility, the existence and latest verdict of a trunk pipeline, and merged-branch cleanup settings. Provider settings alone do not prove branch origin, lifetime, daily integration, or deletion in every case, so branch-lifecycle evidence remains `unverified` until history supplies the missing facts.
