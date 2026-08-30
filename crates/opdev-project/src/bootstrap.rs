use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

const START_MARKER: &str = "<!-- opdev:start -->";
const END_MARKER: &str = "<!-- opdev:end -->";

const AGENTS_BLOCK: &str = r"<!-- opdev:start -->
## OpDev development protocol

This repository is initialized for OpDev. Apply this protocol seamlessly to every software-development task; do not ask whether to use it. `.opdev/project.yaml` is the machine-readable project contract and the starting point for finding project-specific commands, authorities, risks, tests, delivery requirements, assurance profiles, and context routes. Do not assume that design or operational material lives in a conventional `docs/` directory.

If the `opdev` CLI or required OpDev agent integration is unavailable, tell the user before substantive development begins and offer to install it. Do not silently replace OpDev with an improvised process. You may continue only when the user chooses to proceed without it or the current task does not require the unavailable capability.

For each development task:

1. Read `.opdev/project.yaml` before planning or editing. Load the authorities listed by `context.always` and the authorities in every route relevant to the task. Treat those sources as project facts; resolve contradictions explicitly instead of guessing.
2. Identify the intended outcome, acceptance evidence, affected consumers, and applicable quality risks. Use the project work authority for active decisions and progress when one is declared.
3. Keep design effort proportional to risk, reversibility, novelty, and blast radius. Record durable decisions in the declared decision or architecture authority when the project contract routes the task there.
4. Implement in small, reviewable increments that preserve delivered behavior except where the accepted change intentionally modifies it. Integrate with the declared trunk frequently and keep branches short-lived.
5. Add or update automated tests for behavioral changes. Add regression protection for escaped defects unless a specific justification is recorded. Exercise the suites declared for the relevant stage both before and after integration. Keep retries visible; quarantines require an owner, tracked remediation, and an expiry. Use coverage as risk evidence, not as a substitute for meaningful assertions.
6. Run canonical commands from `commands` as argument vectors in their declared working directories. Do not reinterpret them through a shell or substitute a different command without reporting the difference. Project extensions may add checks but may not weaken, replace, or mark core requirements satisfied.
7. Report outcomes using OpDev semantics: `passed`, `failed`, `unverified`, `not_applicable`, `error`, or `migration_required`. Only `passed` and justified `not_applicable` satisfy a required rule. Missing evidence is `unverified`, not a pass. Tooling failure is `error`, not a product failure. Do not claim a gate passed when a required rule has another outcome.

MinimumCD requirements are mandatory. Every change is version controlled and delivered through CI; CI is the exclusive delivery path. Use one integration trunk, stop delivery when it is red, and restore it as the highest priority. Build a deployable artifact once, identify it immutably, and promote that same artifact rather than rebuilding. Qualify in a production-like environment when the project has runtime environments. Keep configuration versioned and tested while externalizing environment-specific values. Delivery must have a single consumer-facing path and an automated, tested recovery strategy appropriate to the software. Preserve or deliberately migrate already-delivered behavior.

Before declaring work complete, reconcile implementation, tests, project authorities, delivery behavior, and tracked work. Run the applicable canonical checks and provide concise evidence, including anything not run or still requiring migration. Never hide a failing or unverified requirement behind a summary success statement.
<!-- opdev:end -->";

const CLAUDE_BLOCK: &str = r"<!-- opdev:start -->
@AGENTS.md
<!-- opdev:end -->";

/// How reconciliation affected one managed file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChange {
    /// A new file was created.
    Created,
    /// An existing managed block was updated.
    Updated,
    /// The current file already contained the desired guidance.
    Unchanged,
}

/// Reconciliation result for one project-level agent instruction file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedFile {
    /// Absolute file path.
    pub path: PathBuf,
    /// Resulting change.
    pub change: FileChange,
}

/// Errors produced while reconciling agent instructions.
#[derive(Debug, Error)]
pub enum BootstrapError {
    /// An instruction file could not be read.
    #[error("could not read agent instructions `{path}`: {source}")]
    Read {
        /// File path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },

    /// An instruction file could not be written.
    #[error("could not write agent instructions `{path}`: {source}")]
    Write {
        /// File path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },

    /// Existing marker structure is ambiguous and cannot be edited safely.
    #[error("agent instructions `{path}` contain malformed or duplicate OpDev markers")]
    MalformedMarkers {
        /// File path.
        path: PathBuf,
    },
}

/// Creates or updates the managed `OpDev` sections in `AGENTS.md` and
/// `CLAUDE.md` while preserving all project-owned content outside the markers.
///
/// # Errors
///
/// Returns [`BootstrapError`] for unreadable files, failed writes, or ambiguous
/// marker layouts. Ambiguous files are never modified.
pub fn reconcile_agent_files(root: &Path) -> Result<Vec<ManagedFile>, BootstrapError> {
    let agents = reconcile_file(&root.join("AGENTS.md"), AGENTS_BLOCK, false)?;
    let claude = reconcile_file(&root.join("CLAUDE.md"), CLAUDE_BLOCK, true)?;
    Ok(vec![agents, claude])
}

fn reconcile_file(
    path: &Path,
    desired_block: &str,
    accept_existing_agents_import: bool,
) -> Result<ManagedFile, BootstrapError> {
    let existing = match fs::read_to_string(path) {
        Ok(value) => Some(value),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(BootstrapError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    let (next, change) = match existing {
        None => (format!("{desired_block}\n"), FileChange::Created),
        Some(ref content)
            if accept_existing_agents_import
                && !content.contains(START_MARKER)
                && content.lines().any(|line| line.trim() == "@AGENTS.md") =>
        {
            (content.clone(), FileChange::Unchanged)
        }
        Some(ref content) => {
            let next = replace_or_append_block(path, content, desired_block)?;
            let change = if next == *content {
                FileChange::Unchanged
            } else {
                FileChange::Updated
            };
            (next, change)
        }
    };

    if change != FileChange::Unchanged {
        fs::write(path, next).map_err(|source| BootstrapError::Write {
            path: path.to_path_buf(),
            source,
        })?;
    }
    Ok(ManagedFile {
        path: path.to_path_buf(),
        change,
    })
}

fn replace_or_append_block(
    path: &Path,
    content: &str,
    desired_block: &str,
) -> Result<String, BootstrapError> {
    let starts: Vec<_> = content.match_indices(START_MARKER).collect();
    let ends: Vec<_> = content.match_indices(END_MARKER).collect();
    match (starts.as_slice(), ends.as_slice()) {
        ([], []) => {
            let newline = newline_style(content);
            let block = desired_block.replace('\n', newline);
            let trimmed = content.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                Ok(format!("{block}{newline}"))
            } else {
                Ok(format!("{trimmed}{newline}{newline}{block}{newline}"))
            }
        }
        ([(start, _)], [(end, _)]) if start < end => {
            let end = end + END_MARKER.len();
            let newline = newline_style(content);
            let block = desired_block.replace('\n', newline);
            let mut next = String::with_capacity(content.len() + block.len());
            next.push_str(&content[..*start]);
            next.push_str(&block);
            next.push_str(&content[end..]);
            Ok(next)
        }
        _ => Err(BootstrapError::MalformedMarkers {
            path: path.to_path_buf(),
        }),
    }
}

fn newline_style(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_both_files_and_is_idempotent() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let first = reconcile_agent_files(directory.path())?;
        assert!(
            first
                .iter()
                .all(|result| result.change == FileChange::Created)
        );
        let agents = fs::read_to_string(directory.path().join("AGENTS.md"))?;
        assert!(agents.contains("If the `opdev` CLI"));
        assert!(agents.contains("MinimumCD requirements are mandatory"));
        assert!(fs::read_to_string(directory.path().join("CLAUDE.md"))?.contains("@AGENTS.md"));

        let second = reconcile_agent_files(directory.path())?;
        assert!(
            second
                .iter()
                .all(|result| result.change == FileChange::Unchanged)
        );
        Ok(())
    }

    #[test]
    fn preserves_project_owned_content() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        fs::write(
            directory.path().join("AGENTS.md"),
            "# Project rules\n\nKeep me.\n",
        )?;
        reconcile_agent_files(directory.path())?;
        let agents = fs::read_to_string(directory.path().join("AGENTS.md"))?;
        assert!(agents.starts_with("# Project rules\n\nKeep me.\n"));
        assert!(agents.contains(START_MARKER));
        Ok(())
    }

    #[test]
    fn upgrades_only_the_managed_block() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        fs::write(
            directory.path().join("AGENTS.md"),
            "before\n<!-- opdev:start -->\nold\n<!-- opdev:end -->\nafter\n",
        )?;
        reconcile_agent_files(directory.path())?;
        let agents = fs::read_to_string(directory.path().join("AGENTS.md"))?;
        assert!(agents.starts_with("before\n"));
        assert!(agents.ends_with("\nafter\n"));
        assert!(!agents.contains("\nold\n"));
        Ok(())
    }

    #[test]
    fn refuses_ambiguous_markers_without_writing() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("AGENTS.md");
        let original = "<!-- opdev:start -->\nbroken\n";
        fs::write(&path, original)?;
        assert!(matches!(
            reconcile_agent_files(directory.path()),
            Err(BootstrapError::MalformedMarkers { .. })
        ));
        assert_eq!(fs::read_to_string(path)?, original);
        Ok(())
    }

    #[test]
    fn accepts_an_existing_claude_import_without_duplication()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        fs::write(
            directory.path().join("CLAUDE.md"),
            "# Claude\n\n@AGENTS.md\n",
        )?;
        reconcile_agent_files(directory.path())?;
        let claude = fs::read_to_string(directory.path().join("CLAUDE.md"))?;
        assert_eq!(claude.matches("@AGENTS.md").count(), 1);
        Ok(())
    }
}
