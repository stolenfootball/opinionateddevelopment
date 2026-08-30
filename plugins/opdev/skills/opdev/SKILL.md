---
name: opdev
description: Use for general software-development work, including planning, design, implementation, debugging, testing, review, CI, release, operations, maintenance, and software documentation. Detect whether the project is initialized with `.opdev/project.yaml`. If initialized, apply OpDev seamlessly. If clearly developing software but uninitialized, ask whether to initialize. If the OpDev CLI is unavailable, inform the user and offer installation. Do not trigger for tasks unrelated to software development.
---

# OpDev

Apply one evidence-driven development loop across software of any language, architecture, platform, or delivery model. Keep project-specific choices in the project contract rather than inventing conventions.

## Establish state

At the start of a software-development task, look for `.opdev/project.yaml` at the Git repository root.

- If it exists, OpDev is initialized. Read it before planning or editing and apply this skill without asking the user whether to use OpDev.
- If it does not exist and the user is clearly asking to develop software, determine whether the `opdev` CLI is available. If it is, ask whether the user wants to initialize OpDev in this project. Do not initialize until they agree.
- If the CLI is unavailable, tell the user that OpDev is not installed and offer the installation choices in [initialization.md](references/initialization.md). Do not install it without consent.
- If the task is not software development, do not interrupt it with an OpDev prompt.

The Claude Code prompt hook may provide the same state as additional context. Treat that as a detection aid, not as a substitute for checking the project contract.

## Work in an initialized project

1. Read `AGENTS.md` and the project contract. `CLAUDE.md` imports the same project guidance for Claude Code.
2. Load authorities selected by `context.always` and by every relevant task route. Follow [project-contract.md](references/project-contract.md) when interpreting fields. Do not assume design material belongs in `docs/`.
3. Use the declared work authority for active status, sequencing, and decisions. Keep static specifications free of roadmap drift.
4. Establish the outcome, scope, exclusions, acceptance conditions, risks, and evidence before substantive edits. Scale design work to risk and reversibility.
5. Make small, reviewable changes. Preserve supported behavior unless the accepted change deliberately migrates it.
6. Apply the testing policy in [testing.md](references/testing.md). Run canonical command argument vectors directly; do not reinterpret them through a shell.
7. For facts the CLI cannot infer, follow [evidence.md](references/evidence.md). Bind change assertions to the exact staged fingerprint; never reuse an assertion after the repository state changes without rechecking it.
8. Use `opdev check` for local evidence. Use `opdev check --ci` in integration CI and `--remote` only when a read-only provider audit is relevant. Report blocked or unavailable evidence honestly.
9. Reconcile implementation, tests, declared authorities, delivery behavior, and tracked work before completion.

Read [workflow.md](references/workflow.md) for the full lifecycle and gate behavior when planning or carrying out a substantive change.

## Preserve the core

MinimumCD requirements are mandatory for every initialized project. Extensions may add or strengthen checks but cannot disable a core rule, change its applicability, replace its result, or suppress required evidence.

Use only these rule outcomes: `passed`, `failed`, `unverified`, `not_applicable`, `error`, and `migration_required`. Only `passed` and justified `not_applicable` satisfy a required rule. Never turn missing evidence, permission failure, tooling failure, or a known migration gap into a pass.
