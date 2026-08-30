#!/usr/bin/env bash
set -eu

project_dir="${CLAUDE_PROJECT_DIR:-$PWD}"

if [ -f "$project_dir/.opdev/project.yaml" ]; then
  printf '%s\n' '{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"This project is initialized for OpDev. For software-development work, invoke the opdev skill, read .opdev/project.yaml and AGENTS.md, and apply them seamlessly without asking whether to use OpDev."}}'
elif command -v opdev >/dev/null 2>&1; then
  printf '%s\n' '{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"The OpDev plugin and CLI are installed, but this project is not initialized. If and only if the user prompt clearly requests software development, ask whether they want to initialize OpDev before substantive development. Do not ask for unrelated tasks."}}'
else
  printf '%s\n' '{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"The OpDev plugin is installed, but the OpDev CLI is unavailable and this project is not initialized. If and only if the user prompt clearly requests software development, inform the user and offer to install OpDev or continue without it. Do not install without consent and do not interrupt unrelated tasks."}}'
fi
