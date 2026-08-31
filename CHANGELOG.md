# Changelog

All notable changes will be recorded here. OpDev follows Semantic Versioning once
the public compatibility boundary stabilizes.

## 0.1.1 - 2026-08-31

- Correct GitHub and GitLab generated installers, GitLab runtime/OAuth behavior,
  Windows npm command shims, evidence bootstrap sizing, Go image selection, and
  CI report isolation discovered by the initial private canaries.
- Add a packaged plugin-to-CLI compatibility contract with fail-closed first-use
  verification in the shared skill and Claude Code prompt hook.
- Make Codex contract validation and strict Claude Code plugin validation
  required CI checks and smoke-test compatibility from the packaged artifact.
- Keep the GitHub native-build mirror synchronized to the qualified revision and
  remove ephemeral `gitlab-release/*` branches after artifact handoff.

## 0.1.0 - 2026-08-30

- Define the 37-rule OpDev and MinimumCD catalog with strict result semantics.
- Add cross-platform Rust project discovery, initialization, checks, and reports.
- Add GitHub Actions and GitLab CI generation, inspection, and read-only audits.
- Add safe project checks, versioned assurance profiles, and bound evidence.
- Add shared Codex and Claude Code plugin behavior with persistent project guidance.
- Add deterministic release checksums, CycloneDX association, and SLSA-compatible
  provenance without a SLSA Build level claim.
- Add deterministic `.tar.gz` and `.zip` packaging with normalized metadata,
  safe path handling, and reproducibility checks.
- Add native release qualification for Windows, Linux GNU, and macOS on x86-64
  and ARM64, plus independently packaged Codex and Claude Code plugin files,
  using an immutable GitHub-builder-to-GitLab-publisher handoff.
- Add keyless Sigstore signatures for every distributed archive and a tested
  safe roll-forward recovery procedure.
