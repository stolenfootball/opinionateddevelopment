# Initialization and installation

## Uninitialized project

Ask once, in plain language: “This looks like software-development work. OpDev is installed but this project is not initialized. Would you like me to initialize it?” Continue the original task without OpDev if the user declines.

After consent:

1. Run `opdev init --dry-run` and show material inferences and migration gaps.
2. If the proposal is reasonable, run `opdev init`.
3. Review `.opdev/project.yaml` with the user where delivery, recovery, coverage, or project kind remains uncertain.
4. Do not overwrite an existing CI configuration. Use `opdev ci generate --provider github|gitlab` for review, then repeat with `--write` only after approval. GitLab generation infers an official image from exact project toolchain metadata; for mixed or custom stacks, review and pass `--image` explicitly. Never accept an image guess that does not contain the project's canonical command toolchain.

Initialization creates `.opdev/project.yaml` and managed sections in `AGENTS.md` and `CLAUDE.md`. It preserves content outside OpDev markers. `opdev upgrade` refreshes only managed guidance.

## CLI unavailable

Inform the user before substantive software development and offer, rather than perform, one of these choices:

- Download the native binary for their platform from the project’s GitLab release page and verify it against `SHA256SUMS`.
- For a Rust development environment, install from the source repository with `cargo install --git https://gitlab.com/stolenfootball-tools/opinionateddevelopment.git --locked opdev-cli`.
- Continue without OpDev for this task.

After installation, verify with `opdev version`; then return to the initialization flow. Never claim installation succeeded without running that check.
