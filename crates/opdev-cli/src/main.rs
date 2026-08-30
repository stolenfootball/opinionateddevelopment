//! `OpDev` command-line entry point.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use opdev_core::{EXTENSION_PROTOCOL_VERSION, PROJECT_SCHEMA_VERSION, RuleId, embedded_catalog};
use opdev_project::{
    CiProvider, CoverageMode, DeliveryStatus, FileChange, MANIFEST_PATH, ProjectManifest,
    RecoveryStrategy, discover, reconcile_agent_files,
};

#[derive(Debug, Parser)]
#[command(name = "opdev", version, about = "Evidence-driven software delivery")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize or reconcile `OpDev` in a software project.
    Init(InitArgs),
    /// Evaluate project requirements.
    Check(CheckArgs),
    /// Explain missing, contradictory, or unverified capabilities.
    Doctor(DoctorArgs),
    /// Upgrade project-owned `OpDev` files explicitly.
    Upgrade(UpgradeArgs),
    /// Show CLI and protocol versions.
    Version,
    /// Inspect the embedded normative rule catalog.
    Rules(RulesArgs),
}

#[derive(Debug, Args)]
struct InitArgs {
    /// Directory inside the Git repository to initialize.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Print the discovered contract without writing files.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct CheckArgs {
    /// Directory inside the initialized Git repository.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Evaluate CI-specific requirements.
    #[arg(long)]
    ci: bool,
    /// Include read-only remote provider auditing.
    #[arg(long)]
    remote: bool,
}

#[derive(Debug, Args)]
struct DoctorArgs {
    /// Directory inside the initialized Git repository.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Include read-only remote provider diagnostics.
    #[arg(long)]
    remote: bool,
}

#[derive(Debug, Args)]
struct UpgradeArgs {
    /// Directory inside the initialized Git repository.
    #[arg(long, default_value = ".")]
    root: PathBuf,
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
        Command::Init(args) => initialize(&args),
        Command::Check(args) => check_project(&args),
        Command::Doctor(args) => doctor(&args),
        Command::Upgrade(args) => upgrade(&args),
    }
}

fn initialize(args: &InitArgs) -> Result<()> {
    let discovery = discover(&args.root).context("could not inspect the repository")?;
    let manifest_path = discovery.root.join(MANIFEST_PATH);

    if manifest_path.exists() {
        ProjectManifest::load(&manifest_path)
            .context("the existing project contract is invalid")?;
        report_agent_changes(&reconcile_agent_files(&discovery.root)?);
        println!(
            "OpDev is already initialized at {}",
            manifest_path.display()
        );
        return Ok(());
    }

    for evidence in &discovery.evidence {
        eprintln!("discovered: {evidence}");
    }
    for warning in &discovery.warnings {
        eprintln!("warning: {warning}");
    }

    if args.dry_run {
        print!("{}", discovery.manifest.to_yaml()?);
    } else {
        discovery.manifest.write_new(&manifest_path)?;
        report_agent_changes(&reconcile_agent_files(&discovery.root)?);
        println!("Initialized OpDev at {}", manifest_path.display());
        println!(
            "Review migration_required and unconfigured values before enforcing delivery gates."
        );
    }
    Ok(())
}

fn upgrade(args: &UpgradeArgs) -> Result<()> {
    let (root, _) = load_project(&args.root)?;
    let changes = reconcile_agent_files(&root)?;
    report_agent_changes(&changes);
    println!("OpDev project-owned guidance is current.");
    Ok(())
}

fn report_agent_changes(changes: &[opdev_project::ManagedFile]) {
    for file in changes {
        let action = match file.change {
            FileChange::Created => "created",
            FileChange::Updated => "updated",
            FileChange::Unchanged => "unchanged",
        };
        println!("{action} {}", file.path.display());
    }
}

fn check_project(args: &CheckArgs) -> Result<()> {
    let (root, manifest) = load_project(&args.root)?;
    println!(
        "passed project contract {}",
        root.join(MANIFEST_PATH).display()
    );
    if args.ci {
        eprintln!("note: CI execution checks are added in Phase 6; the contract itself is valid");
    }
    if args.remote {
        eprintln!("note: read-only remote auditing is added in Phase 7; no remote was queried");
    }
    if manifest.delivery.status == DeliveryStatus::MigrationRequired {
        eprintln!("migration_required: delivery is not yet qualified");
    }
    Ok(())
}

fn doctor(args: &DoctorArgs) -> Result<()> {
    let (root, manifest) = load_project(&args.root)?;
    let mut findings = Vec::new();
    if manifest.project.ci.provider == CiProvider::Unconfigured {
        findings.push("CI provider is unconfigured");
    }
    if manifest.testing.coverage.mode == CoverageMode::Unconfigured {
        findings.push("coverage evidence is unconfigured");
    }
    if manifest.delivery.status == DeliveryStatus::MigrationRequired {
        findings.push("delivery qualification is migration_required");
    }
    if manifest.delivery.recovery.strategy == RecoveryStrategy::Unconfigured {
        findings.push("automated recovery is unconfigured");
    }
    if manifest.commands.is_empty() {
        findings.push("no canonical project commands are declared");
    }

    println!("Project: {}", root.display());
    if findings.is_empty() {
        println!("No project-contract gaps detected.");
    } else {
        for finding in findings {
            println!("- {finding}");
        }
    }
    if args.remote {
        println!("- remote audit not run: read-only provider auditing is added in Phase 7");
    }
    Ok(())
}

fn load_project(start: &Path) -> Result<(PathBuf, ProjectManifest)> {
    let discovery = discover(start).context("could not locate the Git repository")?;
    let manifest_path = discovery.root.join(MANIFEST_PATH);
    if !manifest_path.exists() {
        bail!(
            "OpDev is not initialized; run `opdev init --root {}` first",
            discovery.root.display()
        );
    }
    let manifest = ProjectManifest::load(&manifest_path)
        .with_context(|| format!("could not validate `{}`", manifest_path.display()))?;
    Ok((discovery.root, manifest))
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
            vec!["opdev", "init", "--root", ".", "--dry-run"],
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
