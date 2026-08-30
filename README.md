# OpDev

OpDev is an evidence-driven development system for general software projects.
It combines a Rust CLI, a strict project contract, and one shared agent plugin
for Codex and Claude Code. The workflow is flexible about languages, frameworks,
repository layout, and design-document location; the MinimumCD delivery rules
remain mandatory.

OpDev is currently pre-release (`0.1.0`). Its schemas and rule IDs are versioned,
but installation and compatibility should still be evaluated before broad
organizational rollout.

## What it does

- Detects common Cargo, npm, Python, Go, infrastructure, documentation, and
  plugin projects without executing repository-controlled discovery commands.
- Initializes `.opdev/project.yaml` and persistent `AGENTS.md`/`CLAUDE.md`
  guidance so fresh agents recover the project process automatically.
- Runs canonical tests and project-owned checks through exact argument vectors,
  with no shell interpolation, bounded output, timeouts, and process-tree cleanup.
- Evaluates all 37 core rules with exhaustive outcomes: `passed`, `failed`,
  `unverified`, `not_applicable`, `error`, or `migration_required`.
- Generates and inspects first-class GitHub Actions and GitLab CI configurations.
- Audits GitHub and GitLab policy read-only without treating inaccessible facts
  as passing.
- Binds reviewable change evidence to an exact staged Git fingerprint.
- Generates SHA-256 checksums, a CycloneDX-associated release manifest, and
  SLSA-compatible provenance without claiming a SLSA Build level.

## Install

The CLI and agent plugin are separate on purpose: the plugin provides seamless
agent behavior, while the CLI owns schemas, evaluation, and safe execution. If
the plugin is present but the CLI is missing, the agent tells the developer and
offers installation rather than silently substituting another process.

### CLI from source

Rust 1.97 or newer is required until release archives are published:

```sh
cargo install --locked --git https://gitlab.com/stolenfootball-tools/opinionateddevelopment.git opdev-cli
opdev version
```

Tagged releases publish checksum-verified, keyless Sigstore-signed native archives through GitLab. The
required target contract covers x86-64 and ARM64 on Windows, Linux GNU, and
macOS; Linux musl targets are optional. A release must not claim a required
target until its archive is actually present in that release.

### Codex plugin

```sh
codex plugin marketplace add https://gitlab.com/stolenfootball-tools/opinionateddevelopment.git
codex plugin add opdev@personal
```

Start a new Codex task after installing or updating so the shared skill is
loaded into fresh context.

### Claude Code plugin

```sh
claude plugin marketplace add https://gitlab.com/stolenfootball-tools/opinionateddevelopment.git
claude plugin install opdev@opdev
```

Restart Claude Code or reload plugins after installation. For local development,
`claude --plugin-dir ./plugins/opdev` loads the plugin directly.

## Use

When an installed agent detects a software-development request:

- if `.opdev/project.yaml` exists, it applies OpDev without interrupting the
  developer;
- if the CLI exists but the repository is uninitialized, it asks whether to run
  initialization; and
- if the CLI is missing, it explains that and offers installation.

Manual initialization is also available:

```sh
opdev init --dry-run
opdev init
```

Review the inferred contract, especially `migration_required` delivery and
recovery fields. OpDev deliberately does not invent an artifact, production-like
environment, or recovery strategy.

Typical commands are:

```sh
opdev check
opdev check --ci --format json
opdev doctor --remote
opdev ci inspect
opdev ci generate --provider gitlab
opdev rules --id MCD-TRUNK-001
opdev profiles
```

For evidence that automation cannot infer, stage every material change and run:

```sh
opdev evidence fingerprint
```

Bind reviewed assertions to that value in `.opdev/evidence.yaml`. Future file,
mode, or content changes produce a different fingerprint and invalidate the
change assertions. See [`spec/evidence-ledger.md`](spec/evidence-ledger.md).

## Design boundaries

- `.opdev/project.yaml` is the small machine-readable project contract; it points
  to existing project authorities instead of forcing content into `docs/`.
- `AGENTS.md` is intentionally detailed for reliable recovery by fresh agents.
  `CLAUDE.md` imports it with `@AGENTS.md`, leaving one managed source of behavior.
- Built-in extensions are declarative. Project command checks use shell-free
  argument vectors and a strict JSON protocol. Native dynamic libraries and a
  general policy language are outside version 1.
- GitHub and GitLab are first-class adapters behind public Rust traits. Unknown
  providers remain unverified until an adapter supplies evidence.
- Assurance profiles are exact-version mappings. Selecting a NIST, OpenSSF,
  SLSA, or CycloneDX profile is not a certification or conformance claim.

The normative sources are [`rules/core.yaml`](rules/core.yaml) and
[`spec/README.md`](spec/README.md). Result semantics are defined in
[`spec/result-semantics.md`](spec/result-semantics.md).

## Development

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --release --locked
```

The repository dogfoods OpDev through `.opdev/project.yaml`, GitLab CI, and a
GitHub mirror workflow. See [`CONTRIBUTING.md`](CONTRIBUTING.md) for change and
evidence expectations, and [`SECURITY.md`](SECURITY.md) for private vulnerability
reporting.

Licensed under Apache-2.0.
