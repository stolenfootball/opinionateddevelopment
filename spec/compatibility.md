# Compatibility and versioning

OpDev versions four public contracts independently.

## CLI and plugin release

The CLI and agent plugins use semantic versions. A published plugin declares the
CLI version range it supports in packaged `opdev-compatibility.json`. The shared
plugin skill verifies this relationship before its first OpDev action in each
task. Claude Code also verifies it through the prompt hook. A missing, malformed,
unsupported, or unsatisfied compatibility contract prevents OpDev activation;
Codex plugin installation itself does not provide a portable activation hook.

Pre-1.0 releases may change command-line and plugin behavior between minor
versions, but migrations and diagnostics are still required for project-owned
state.

## Project-manifest schema

`.opdev/project.yaml` contains an integer `schema` version.

- Additive fields that old clients can safely ignore do not change the schema
  version.
- A semantic change, removed field, renamed field, or stricter interpretation
  increments the schema version.
- A CLI MUST refuse to rewrite a newer unsupported schema.
- `opdev upgrade` MUST be explicit, idempotent, and preserve unrelated project
  content.

The first stable CLI will read its current schema and at least one previous
schema when a deterministic migration exists.

## Rule catalog

The catalog has its own integer `catalog_version`. Rule IDs are permanent.

- Clarifications that do not change required behavior keep the rule ID.
- A changed requirement receives a new rule ID; the previous rule remains
  available for interpreting historical evidence.
- Removing a core requirement requires a major OpDev release and a published
  rationale.
- Generated documentation, diagnostics, profiles, and evidence refer to rule IDs
  rather than copied rule prose.

## Extension-result protocol

Project commands and future rule packs communicate through a semantic protocol
version. Major versions are incompatible. Unknown fields in a compatible major
version are ignored unless the protocol explicitly marks them critical.

## External assurance profiles

External standards and profiles are pinned by name and version. Installing a new
OpDev release MUST NOT silently change an existing project's selected profile
version. Profile upgrades are explicit and report newly applicable or changed
requirements before modifying the project contract.

## Provider APIs

GitHub and GitLab adapters isolate provider API versions from the core domain
model. Provider changes may produce `unverified` or `error`; they MUST NOT cause
a remote policy to be reported as passing based on cached assumptions.

