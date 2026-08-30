# Read-only remote audits

`opdev check --remote` and `opdev doctor --remote` query GitHub or GitLab with GET requests only. OpDev does not create pipelines, change branch protection, modify merge settings, delete branches, post statuses, or otherwise mutate a remote repository. Local CI generation remains a separate explicit operation.

Remote URLs are accepted only for the first-class `github.com` and `gitlab.com` hosts in 0.1. This prevents a repository-controlled remote from turning the audit into a request to an arbitrary host. Self-managed instances and additional providers require a future adapter with an explicit trusted API origin.

Authentication is optional. Tokens are read from provider environment variables in a documented precedence order, added only to request headers, and never copied into evidence or diagnostics. Missing permission, unavailable fields, HTTP failures, and non-definitive pipeline states produce `unverified`; they do not become passes. A missing trunk pipeline is `migration_required`, while a definitive failing trunk pipeline is `failed`.

The audit verifies the provider default branch, trunk protection visibility, the existence and latest verdict of a trunk pipeline, and merged-branch cleanup settings. Provider settings alone do not prove branch origin, lifetime, daily integration, or deletion in every case, so branch-lifecycle evidence remains `unverified` until history supplies the missing facts.
