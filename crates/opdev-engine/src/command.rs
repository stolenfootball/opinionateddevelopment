use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use command_group::{CommandGroup, GroupChild};
use opdev_project::CommandSpec;
use thiserror::Error;

const DEFAULT_TIMEOUT_SECONDS: u64 = 900;
const MAX_CAPTURE_BYTES: usize = 64 * 1024;

/// Captured result of one shell-free command execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Execution {
    /// Process exit code, or `None` when terminated by a signal or timeout.
    pub exit_code: Option<i32>,
    /// Whether `OpDev` terminated the process after its deadline.
    pub timed_out: bool,
    /// Captured standard output, bounded to protect the report.
    pub stdout: String,
    /// Captured standard error, bounded to protect the report.
    pub stderr: String,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u128,
}

/// Failures that prevent a canonical command from producing an exit status.
#[derive(Debug, Error)]
pub enum CommandError {
    /// The argument vector was empty.
    #[error("canonical command has an empty argument vector")]
    Empty,
    /// The command could not be started.
    #[error("could not start `{program}`: {source}")]
    Spawn {
        /// Requested executable.
        program: String,
        /// Operating-system error.
        source: std::io::Error,
    },
    /// A bounded output capture could not be created or cloned.
    #[error("could not prepare command {stream} capture: {source}")]
    Capture {
        /// Stream name.
        stream: &'static str,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// Extension input could not be written.
    #[error("could not write command input: {0}")]
    Stdin(std::io::Error),
    /// Waiting for or terminating the child failed.
    #[error("could not wait for command: {0}")]
    Wait(std::io::Error),
    /// A bounded output capture could not be read.
    #[error("could not read command {stream}: {source}")]
    Read {
        /// Stream name.
        stream: &'static str,
        /// Read error.
        source: std::io::Error,
    },
}

/// Executes a validated canonical command directly, never through a shell.
///
/// Optional bytes are written to standard input for the extension protocol.
/// Standard output and error are captured concurrently and bounded.
///
/// # Errors
///
/// Returns [`CommandError`] when the process cannot be started, supplied input
/// cannot be written, waiting fails, or a captured pipe cannot be read.
pub fn execute(
    root: &Path,
    command: &CommandSpec,
    input: Option<&[u8]>,
) -> Result<Execution, CommandError> {
    let Some((program, arguments)) = command.argv.split_first() else {
        return Err(CommandError::Empty);
    };
    let working_directory = command
        .working_directory
        .as_deref()
        .map_or_else(|| root.to_path_buf(), |directory| root.join(directory));
    let mut stdout_capture = tempfile::tempfile().map_err(|source| CommandError::Capture {
        stream: "stdout",
        source,
    })?;
    let stdout_writer = stdout_capture
        .try_clone()
        .map_err(|source| CommandError::Capture {
            stream: "stdout",
            source,
        })?;
    let mut stderr_capture = tempfile::tempfile().map_err(|source| CommandError::Capture {
        stream: "stderr",
        source,
    })?;
    let stderr_writer = stderr_capture
        .try_clone()
        .map_err(|source| CommandError::Capture {
            stream: "stderr",
            source,
        })?;

    let mut process = Command::new(program);
    process
        .args(arguments)
        .current_dir(working_directory)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::from(stdout_writer))
        .stderr(Stdio::from(stderr_writer));

    let started = Instant::now();
    let mut child = process
        .group_spawn()
        .map_err(|source| CommandError::Spawn {
            program: program.clone(),
            source,
        })?;
    if let Some(input) = input {
        let stdin = child.inner().stdin.take();
        if let Some(mut stdin) = stdin
            && let Err(source) = stdin.write_all(input)
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CommandError::Stdin(source));
        }
    }

    let timeout = Duration::from_secs(command.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS));
    let (status, timed_out) = wait_with_timeout(&mut child, timeout)?;
    let stdout = read_capture(&mut stdout_capture, "stdout")?;
    let stderr = read_capture(&mut stderr_capture, "stderr")?;
    Ok(Execution {
        exit_code: status.code(),
        timed_out,
        stdout: bounded_text(&stdout),
        stderr: bounded_text(&stderr),
        duration_ms: started.elapsed().as_millis(),
    })
}

fn wait_with_timeout(
    child: &mut GroupChild,
    timeout: Duration,
) -> Result<(ExitStatus, bool), CommandError> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(CommandError::Wait)? {
            return Ok((status, false));
        }
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            child.kill().map_err(CommandError::Wait)?;
            return child
                .wait()
                .map(|status| (status, true))
                .map_err(CommandError::Wait);
        }
        thread::sleep(Duration::from_millis(25).min(timeout.saturating_sub(elapsed)));
    }
}

fn read_capture(file: &mut File, stream: &'static str) -> Result<Vec<u8>, CommandError> {
    file.rewind()
        .and_then(|()| {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            Ok(bytes)
        })
        .map_err(|source| CommandError::Read { stream, source })
}

fn bounded_text(bytes: &[u8]) -> String {
    let suffix = b"\n[output truncated by OpDev]";
    if bytes.len() <= MAX_CAPTURE_BYTES {
        String::from_utf8_lossy(bytes).into_owned()
    } else {
        let retained = MAX_CAPTURE_BYTES.saturating_sub(suffix.len());
        let mut bounded = bytes[..retained].to_vec();
        bounded.extend_from_slice(suffix);
        String::from_utf8_lossy(&bounded).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_missing_program_without_a_shell_fallback() {
        let command = CommandSpec {
            argv: vec!["opdev-program-that-cannot-exist-7e26a8".into(), "&&".into()],
            working_directory: None,
            timeout_seconds: Some(1),
        };
        assert!(matches!(
            execute(Path::new("."), &command, None),
            Err(CommandError::Spawn { .. })
        ));
    }

    #[test]
    fn output_capture_is_bounded() {
        let oversized = vec![b'a'; MAX_CAPTURE_BYTES * 2];
        let bounded = bounded_text(&oversized);
        assert!(bounded.len() <= MAX_CAPTURE_BYTES);
        assert!(bounded.ends_with("[output truncated by OpDev]"));
    }

    #[test]
    fn timeout_terminates_the_command_group() -> Result<(), Box<dyn std::error::Error>> {
        let executable = std::env::current_exe()?;
        let command = CommandSpec {
            argv: vec![
                executable.display().to_string(),
                "--exact".into(),
                "command::tests::timeout_helper".into(),
                "--ignored".into(),
            ],
            working_directory: None,
            timeout_seconds: Some(1),
        };
        let started = Instant::now();
        let result = execute(Path::new("."), &command, None)?;
        assert!(result.timed_out);
        assert!(started.elapsed() < Duration::from_secs(4));
        Ok(())
    }

    #[test]
    #[ignore = "subprocess helper for timeout_terminates_the_command_group"]
    fn timeout_helper() {
        thread::sleep(Duration::from_secs(10));
    }
}
