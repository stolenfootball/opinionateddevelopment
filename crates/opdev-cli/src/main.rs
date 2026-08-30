//! `OpDev` command-line entry point.

use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use opdev_core::{EXTENSION_PROTOCOL_VERSION, PROJECT_SCHEMA_VERSION, RuleId, embedded_catalog};

#[derive(Debug, Parser)]
#[command(name = "opdev", version, about = "Evidence-driven software delivery")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize or reconcile `OpDev` in a software project.
    Init,
    /// Evaluate project requirements.
    Check(CheckArgs),
    /// Explain missing, contradictory, or unverified capabilities.
    Doctor(DoctorArgs),
    /// Upgrade project-owned `OpDev` files explicitly.
    Upgrade,
    /// Show CLI and protocol versions.
    Version,
    /// Inspect the embedded normative rule catalog.
    Rules(RulesArgs),
}

#[derive(Debug, Args)]
struct CheckArgs {
    /// Evaluate CI-specific requirements.
    #[arg(long)]
    ci: bool,
    /// Include read-only remote provider auditing.
    #[arg(long)]
    remote: bool,
}

#[derive(Debug, Args)]
struct DoctorArgs {
    /// Include read-only remote provider diagnostics.
    #[arg(long)]
    remote: bool,
}

#[derive(Debug, Args)]
struct RulesArgs {
    /// Show one stable rule ID instead of listing the catalog.
    #[arg(long)]
    id: Option<RuleId>,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Version => {
            let catalog = embedded_catalog().context("could not load the embedded rule catalog")?;
            println!("opdev {}", env!("CARGO_PKG_VERSION"));
            println!("project schema {PROJECT_SCHEMA_VERSION}");
            println!("rule catalog {}", catalog.catalog_version);
            println!("extension protocol {EXTENSION_PROTOCOL_VERSION}");
            Ok(())
        }
        Command::Rules(args) => show_rules(args),
        Command::Init => not_implemented("init", "Phase 3"),
        Command::Check(args) => {
            let detail = match (args.ci, args.remote) {
                (true, true) => "check --ci --remote",
                (true, false) => "check --ci",
                (false, true) => "check --remote",
                (false, false) => "check",
            };
            not_implemented(detail, "Phases 3 through 7")
        }
        Command::Doctor(args) => {
            let detail = if args.remote {
                "doctor --remote"
            } else {
                "doctor"
            };
            not_implemented(detail, "Phases 3 through 7")
        }
        Command::Upgrade => not_implemented("upgrade", "Phase 4"),
    }
}

fn show_rules(args: RulesArgs) -> Result<()> {
    let catalog = embedded_catalog().context("could not load the embedded rule catalog")?;
    if let Some(id) = args.id {
        let rule = catalog
            .find(&id)
            .with_context(|| format!("the embedded catalog does not contain `{id}`"))?;
        println!("{}: {}", rule.id, rule.title);
        println!("{}", rule.statement);
    } else {
        for rule in catalog.rules {
            println!("{}\t{}", rule.id, rule.title);
        }
    }
    Ok(())
}

fn not_implemented(command: &str, phase: &str) -> Result<()> {
    bail!("`opdev {command}` is reserved for {phase} and is not implemented in this build")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn command_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn final_command_surface_parses() {
        for arguments in [
            vec!["opdev", "init"],
            vec!["opdev", "check", "--ci", "--remote"],
            vec!["opdev", "doctor", "--remote"],
            vec!["opdev", "upgrade"],
            vec!["opdev", "version"],
            vec!["opdev", "rules", "--id", "MCD-TRUNK-001"],
        ] {
            assert!(Cli::try_parse_from(arguments).is_ok());
        }
    }
}
