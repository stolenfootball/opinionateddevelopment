# OpDev

**Evidence-driven software development for humans and coding agents.**

OpDev gives a Git-backed software project a repeatable path from an intended
outcome to a tested, reviewable, deliverable change. It keeps languages,
frameworks, repository layout, work tracking, and design-document location
project-specific while enforcing the delivery rules from
[MinimumCD](https://minimumcd.org/) and strict, fail-closed evidence semantics.

**Project status:** OpDev `0.1.0` is the initial release line. Its public
compatibility boundary is pre-1.0, while rule IDs, schemas, and assurance
profiles are independently versioned so changes remain explicit and reviewable.

## Why OpDev

- **Process continuity:** Codex and Claude Code recover the same project-owned
  workflow when an agent starts with fresh context.
- **Project independence:** OpDev points to the repository's existing
  architecture, contracts, tests, and work tracker instead of imposing a
  `docs/` layout or a particular development framework.
- **Honest gates:** Missing evidence is `unverified`, tooling problems are
  `error`, and known adoption gaps are `migration_required`; none is silently
  converted into a pass.
- **Testing as a contract:** Canonical project commands, change tests, regression
  expectations, flake policy, and selected risks are evaluated together.
- **Continuous delivery discipline:** MinimumCD rules remain mandatory even when
  a project adopts the rest of the process incrementally.
- **Portable automation:** The Rust CLI, GitHub Actions adapter, GitLab CI
  adapter, and shell-free extension protocol are designed to work across
  software ecosystems.

OpDev does not certify a project, replace engineering judgment, or claim that an
artifact is deployable before the project proves its build, delivery, and
recovery path.

## How it works

OpDev models a general software lifecycle:

```text
Understand -> Specify -> Design -> Implement -> Verify
           -> Integrate -> Package -> Deliver -> Observe -> Learn
```

1. `opdev init` discovers safe, static project facts and writes a small project
   contract.
2. The contract points to existing authorities and exact project commands.
3. Installed agents use the contract and persistent repository guidance without
   asking the developer to restate the process in every task.
4. `opdev check` executes configured checks and evaluates every applicable rule.
5. Reviewed facts that cannot be inferred safely can be bound to the exact staged
   Git index in `.opdev/evidence.yaml`.
6. CI evaluates the integration gate; release automation binds an already-built
   artifact to checksums, an SBOM, a manifest, and provenance.

Agent behavior is intentionally low-friction: an initialized project uses OpDev
without interruption; an uninitialized software project prompts before running
`opdev init`; and a missing CLI is reported with an offer to install it.

## Quick start

### 1. Install the CLI

Download the archive for your system from the
[v0.1.0 release](https://gitlab.com/stolenfootball-tools/opinionateddevelopment/-/releases/v0.1.0),
verify it as described in [`release/README.md`](release/README.md), and place the
`opdev` executable on `PATH`.

Linux x86-64 example:

```sh
curl --fail --location --output opdev.tar.gz \
  https://gitlab.com/stolenfootball-tools/opinionateddevelopment/-/releases/v0.1.0/downloads/opdev-0.1.0-x86_64-unknown-linux-gnu.tar.gz
tar -xzf opdev.tar.gz
install -m 0755 opdev "$HOME/.local/bin/opdev"
opdev version
```

Windows x86-64 example:

```powershell
Invoke-WebRequest `
  https://gitlab.com/stolenfootball-tools/opinionateddevelopment/-/releases/v0.1.0/downloads/opdev-0.1.0-x86_64-pc-windows-msvc.zip `
  -OutFile opdev.zip
Expand-Archive opdev.zip -DestinationPath opdev
.\opdev\opdev.exe version
```

Archives are published for Windows, Linux GNU, and macOS on both x86-64 and
ARM64. Rust 1.97 or newer provides a source-install fallback:

```sh
cargo install --locked --git https://gitlab.com/stolenfootball-tools/opinionateddevelopment.git opdev-cli
opdev version
```

The CLI owns project discovery, schemas, command execution, rule evaluation,
reports, provider inspection, and release evidence.

### 2. Install the agent plugin

The plugin is optional for CLI-only use. Install it when you want Codex or
Claude Code to apply OpDev automatically during software-development work.

For Codex:

```sh
codex plugin marketplace add https://gitlab.com/stolenfootball-tools/opinionateddevelopment.git
codex plugin add opdev@personal
```

Start a new Codex task after installation or update so the skill is loaded into
fresh context.

For Claude Code:

```sh
claude plugin marketplace add https://gitlab.com/stolenfootball-tools/opinionateddevelopment.git
claude plugin install opdev@opdev
```

Restart Claude Code or reload its plugins after installation. Plugin developers
can load this checkout directly with `claude --plugin-dir ./plugins/opdev`.

### 3. Initialize a repository

Run the dry run first. Discovery does not execute repository-controlled
commands.

```sh
cd path/to/your-project
opdev init --dry-run
opdev init
opdev check
```

Review `.opdev/project.yaml`, especially values marked `migration_required` or
`unconfigured`. OpDev deliberately does not invent a production-like
environment, artifact, coverage target, or recovery strategy.

Commit the project-owned process files when the inferred contract is correct:

```sh
git add .opdev/project.yaml AGENTS.md CLAUDE.md
git commit -m "chore: initialize OpDev"
```

### 4. Make a change

After initialization, work with an agent or your usual tools. Run the canonical
checks before staging the complete change:

```sh
opdev check
git add -- path/to/changed-file
```

If OpDev reports a required fact that automation cannot verify, review the fact,
fingerprint the complete staged index, and add only justified assertions to the
evidence ledger:

```sh
opdev evidence fingerprint
# Add a matching change entry to .opdev/evidence.yaml.
git add .opdev/evidence.yaml
opdev check --ci
git commit -m "feat: describe the change"
git push
```

Skip the evidence step when no reviewed assertion is needed. Changing any staged
path, content, or executable bit invalidates an existing fingerprint. The pull
or merge request then runs the project's normal build and test commands plus the
OpDev integration gate.

### 5. Connect CI

Inspect an existing first-class configuration:

```sh
opdev ci inspect
```

If the project does not have one, generate a pinned baseline for GitLab CI or
GitHub Actions:

```sh
opdev ci generate --provider gitlab --write
# Or: opdev ci generate --provider github --write
```

For GitLab, OpDev infers an official toolchain image when the repository pins
Rust (`rust-toolchain.toml` or `rust-toolchain`), Go (`go.mod`), Node.js
(`.nvmrc` or `.node-version`), or Python (`.python-version`). Mixed or custom
stacks can select a reviewed image explicitly:

```sh
opdev ci generate --provider gitlab --image registry.example.com/team/toolchain:2026.08 --write
```

The image must contain the project toolchain plus a POSIX shell, Git, curl,
tar, `sha256sum`, and `mktemp`. OpDev refuses to guess when it cannot infer a
compatible image. The generated job verifies these prerequisites, downloads
and checksum-verifies the exact OpDev release outside the checkout, confirms
that the CLI starts, and then evaluates the project contract.

Generation refuses to replace an existing provider configuration. Review and
commit the generated file like any other build-system change.

## Rules, results, and gates

OpDev `0.1.0` evaluates 37 core OpDev and MinimumCD rules. Every applicable rule
has exactly one result: `passed`, `failed`, `unverified`, `not_applicable`,
`error`, or `migration_required`. Only `passed` and justified
`not_applicable` satisfy a required rule.

Rules and configured checks contribute to four independent gates:

| Gate | Decision |
| --- | --- |
| Development | Whether ordinary local implementation may proceed. |
| Integration | Whether a change may enter trunk. |
| Delivery | Whether an identified artifact may be delivered through the declared automated path. |
| Compliance | Whether the project may claim its selected OpDev or external assurance profile. |

`migration_required` supports incremental adoption, but it cannot qualify a
delivery or support a compliance claim. OpDev also keeps correctness,
deployability, and real-world effectiveness as separate judgments.

The normative sources are [`rules/core.yaml`](rules/core.yaml) and
[`spec/README.md`](spec/README.md). Exact aggregation behavior is defined in
[`spec/result-semantics.md`](spec/result-semantics.md).

## Project-owned files

| Path | Purpose |
| --- | --- |
| `.opdev/project.yaml` | Small, schema-validated project contract that selects authorities, commands, delivery behavior, and assurance profiles. |
| `.opdev/evidence.yaml` | Optional reviewed project and staged-change assertions; it is evidence ingress, not a waiver file. |
| `AGENTS.md` | Detailed managed guidance that gives fresh agents reliable process continuity. Existing project-owned content is preserved. |
| `CLAUDE.md` | Imports the shared `AGENTS.md` guidance for Claude Code while preserving one behavioral source. |

Initialization is idempotent and updates only OpDev-managed blocks. Existing
project documentation stays where the project already keeps it.

## Supported surface

| Capability | Version 0.1 support |
| --- | --- |
| Project discovery | Cargo, npm, Python, Go, Terraform/infrastructure, documentation, and agent-plugin repositories. |
| Agent integration | Codex and Claude Code through one shared skill package. |
| CI providers | GitHub Actions and GitLab CI as first-class adapters. |
| Remote audit | Read-only GitHub and GitLab policy and pipeline inspection. Unknown or inaccessible facts remain unverified. |
| Extensions | Declarative checks using exact argument vectors and a strict JSON protocol. Extensions can strengthen gates but cannot replace core results. |
| Release evidence | SHA-256 checksums, CycloneDX SBOM association, a release manifest, and SLSA-compatible provenance without a SLSA Build-level claim. |
| Native release contract | Windows, Linux GNU, and macOS on x86-64 and ARM64; optional Linux musl targets. A target is not supported until its archive is present in a release. |

Other languages and build systems can be configured explicitly in
`.opdev/project.yaml`; discovery support is an ergonomic baseline, not an
allowlist of software OpDev can govern.

## CLI reference

| Command | Purpose |
| --- | --- |
| `opdev init` | Discover and initialize or reconcile project-owned OpDev files. |
| `opdev check` | Execute configured checks and evaluate project requirements. |
| `opdev doctor` | Explain missing, contradictory, or unverified capabilities. |
| `opdev ci` | Generate or inspect GitHub Actions and GitLab CI configurations. |
| `opdev evidence` | Fingerprint staged repository state for reviewed change evidence. |
| `opdev rules` | Inspect the embedded normative rule catalog. |
| `opdev profiles` | Inspect bundled exact-version assurance profiles. |
| `opdev release` | Deterministically package already-built artifacts and bind them to checksums, an SBOM, source, and provenance. |
| `opdev upgrade` | Explicitly upgrade project-owned OpDev files. |

Use `opdev <command> --help` for the complete command surface. JSON reports are
available through `opdev check --format json`; CI mode uses
`opdev check --ci --format json`.

## Security and trust boundaries

OpDev treats initialized project content as untrusted. Discovery is static,
configured checks use exact argument vectors without a general-purpose shell,
command output and runtime are bounded, remote audits are read-only, and
extensions cannot weaken core results. Running a project's configured checks
still executes code selected by that project, so review `.opdev/project.yaml`
before checking an untrusted repository.

Report suspected vulnerabilities privately as described in
[`SECURITY.md`](SECURITY.md).

## Documentation

| Topic | Reference |
| --- | --- |
| Normative model and authority order | [`spec/README.md`](spec/README.md) |
| Result and gate semantics | [`spec/result-semantics.md`](spec/result-semantics.md) |
| Project evidence ledger | [`spec/evidence-ledger.md`](spec/evidence-ledger.md) |
| CI provider boundaries | [`spec/ci-providers.md`](spec/ci-providers.md) |
| Remote audits | [`spec/remote-audits.md`](spec/remote-audits.md) |
| Extension protocol | [`spec/extensions.md`](spec/extensions.md) |
| Assurance profiles | [`spec/assurance-profiles.md`](spec/assurance-profiles.md) |
| Compatibility policy | [`spec/compatibility.md`](spec/compatibility.md) |
| Release evidence | [`spec/release-evidence.md`](spec/release-evidence.md) |

## Contributing and help

Use [GitLab issues](https://gitlab.com/stolenfootball-tools/opinionateddevelopment/-/issues)
for questions, defects, and proposed improvements. Focused contributions are
welcome; start with [`CONTRIBUTING.md`](CONTRIBUTING.md) and keep behavioral or
architectural changes grounded in the normative specification.

The repository requires Rust 1.97. Run the canonical checks before opening a
merge request:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --release --locked
```

Notable changes are recorded in [`CHANGELOG.md`](CHANGELOG.md). OpDev is
maintained by Opinionated Development contributors and is available under the
[Apache License 2.0](LICENSE).
