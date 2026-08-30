# OpDev agent plugin

This directory is one shared skill package for Codex and Claude Code. Codex reads `.codex-plugin/plugin.json`; Claude Code reads `.claude-plugin/plugin.json`, the same `skills/opdev/SKILL.md`, and a `UserPromptSubmit` hook that contributes only initialization state.

The hook does not enforce policy or modify the project. In an initialized repository, `.opdev/project.yaml` and the managed `AGENTS.md` block remain authoritative. In an uninitialized repository, the skill asks before running `opdev init`. If the CLI is unavailable, it offers installation rather than attempting it.

For local Claude Code testing, run `claude --plugin-dir ./plugins/opdev`. Codex plugin installation and release packaging are described at the repository root.
