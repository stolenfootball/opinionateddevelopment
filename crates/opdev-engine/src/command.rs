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

    let mut process = command_for(program, arguments);
    process
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

fn command_for(program: &str, arguments: &[String]) -> Command {
    #[cfg(windows)]
    let executable = resolve_package_manager_shim(
        program,
        std::env::var_os("PATH").as_deref(),
        std::env::var_os("PATHEXT").as_deref(),
    )
    .unwrap_or_else(|| program.into());
    #[cfg(not(windows))]
    let executable = program;

    let mut command = Command::new(executable);
    command.args(arguments);
    command
}

#[cfg(windows)]
fn resolve_package_manager_shim(
    program: &str,
    search_path: Option<&std::ffi::OsStr>,
    path_extensions: Option<&std::ffi::OsStr>,
) -> Option<std::path::PathBuf> {
    const TRUSTED_NAMES: &[&str] = &["npm", "npx", "pnpm", "pnpx", "yarn", "yarnpkg"];
    const EXECUTABLE_EXTENSIONS: &[&str] = &[".COM", ".EXE", ".BAT", ".CMD"];
    const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

    let requested = Path::new(program);
    if requested.components().count() != 1 {
        return None;
    }
    let extension = requested
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .map(|value| format!(".{}", value.to_ascii_uppercase()));
    if extension
        .as_deref()
        .is_some_and(|value| !EXECUTABLE_EXTENSIONS.contains(&value))
    {
        return None;
    }
    let name = requested
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)?
        .to_ascii_lowercase();
    if !TRUSTED_NAMES.contains(&name.as_str()) {
        return None;
    }

    let extensions = extension.map_or_else(
        || {
            path_extensions
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or(DEFAULT_PATHEXT)
                .split(';')
                .filter_map(|value| {
                    let value = value.trim().to_ascii_uppercase();
                    EXECUTABLE_EXTENSIONS
                        .contains(&value.as_str())
                        .then_some(value)
                })
                .collect::<Vec<_>>()
        },
        |value| vec![value],
    );
    let search_path = search_path?;
    for directory in std::env::split_paths(search_path) {
        for extension in &extensions {
            let candidate = directory.join(format!("{name}{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
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

    #[cfg(windows)]
    #[test]
    fn trusted_package_manager_shims_follow_path_and_pathext()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        std::fs::create_dir_all(&first)?;
        std::fs::create_dir_all(&second)?;
        for name in ["npm", "npx", "pnpm", "pnpx", "yarn", "yarnpkg"] {
            std::fs::write(second.join(format!("{name}.CMD")), "@exit /b 0\r\n")?;
        }
        std::fs::write(first.join("npm.EXE"), b"fixture")?;
        std::fs::write(first.join("npm.PS1"), b"fixture")?;
        std::fs::write(first.join("cargo.CMD"), b"fixture")?;
        let search_path = std::env::join_paths([&first, &second])?;
        let path_extensions = std::ffi::OsStr::new(".PS1;.EXE;.CMD");

        assert_eq!(
            resolve_package_manager_shim("npm", Some(&search_path), Some(path_extensions)),
            Some(first.join("npm.EXE"))
        );
        for name in ["npx", "pnpm", "pnpx", "yarn", "yarnpkg"] {
            assert_eq!(
                resolve_package_manager_shim(name, Some(&search_path), Some(path_extensions)),
                Some(second.join(format!("{name}.CMD")))
            );
        }
        assert!(
            resolve_package_manager_shim("cargo", Some(&search_path), Some(path_extensions))
                .is_none()
        );
        assert!(
            resolve_package_manager_shim("npm.ps1", Some(&search_path), Some(path_extensions))
                .is_none()
        );
        assert!(
            resolve_package_manager_shim(".\\npm", Some(&search_path), Some(path_extensions))
                .is_none()
        );
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn initialized_node_project_runs_npm_canonical_check() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempfile::tempdir()?;
        let root = directory.path();
        let status = Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(root)
            .status()?;
        assert!(status.success());
        std::fs::write(
            root.join("package.json"),
            r#"{
  "name": "opdev-windows-npm-fixture",
  "version": "1.0.0",
  "scripts": {
    "test": "node -e \"console.log('opdev npm fixture passed')\""
  }
}
"#,
        )?;
        let discovery = opdev_project::discover(root)?;
        let manifest_path = root.join(opdev_project::MANIFEST_PATH);
        discovery.manifest.write_new(&manifest_path)?;
        let initialized = opdev_project::ProjectManifest::load(&manifest_path)?;
        let report = crate::evaluate(root, &initialized, crate::CheckOptions::pre_merge())?;
        let check = report
            .checks
            .iter()
            .find(|check| check.id == "check")
            .ok_or_else(|| std::io::Error::other("Node check suite was not evaluated"))?;
        assert_eq!(check.outcome, opdev_core::Outcome::Passed);
        assert!(
            check
                .stdout
                .as_deref()
                .is_some_and(|stdout| stdout.contains("opdev npm fixture passed"))
        );
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn package_manager_shim_arguments_cannot_escape_into_cmd()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let marker = directory.path().join("shell-escape-marker");
        let command = CommandSpec {
            argv: vec![
                "npm".into(),
                format!("& echo escaped > {}", marker.display()),
            ],
            working_directory: None,
            timeout_seconds: Some(10),
        };

        let execution = execute(directory.path(), &command, None);
        assert!(
            execution.is_err() || execution.is_ok_and(|result| result.exit_code != Some(0)),
            "an injection-shaped npm argument unexpectedly succeeded"
        );
        assert!(!marker.exists(), "npm argument escaped into cmd.exe");
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn installed_package_manager_shims_execute_where_available()
    -> Result<(), Box<dyn std::error::Error>> {
        let search_path = std::env::var_os("PATH");
        let path_extensions = std::env::var_os("PATHEXT");
        let mut executed = Vec::new();
        for program in ["npm", "npx", "pnpm", "pnpx", "yarn", "yarnpkg"] {
            if resolve_package_manager_shim(
                program,
                search_path.as_deref(),
                path_extensions.as_deref(),
            )
            .is_none()
            {
                continue;
            }
            let command = CommandSpec {
                argv: vec![program.into(), "--version".into()],
                working_directory: None,
                timeout_seconds: Some(30),
            };
            let result = execute(Path::new("."), &command, None)?;
            assert_eq!(result.exit_code, Some(0), "{program}: {}", result.stderr);
            executed.push(program);
        }
        assert!(executed.contains(&"npm"));
        assert!(executed.contains(&"npx"));
        Ok(())
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
