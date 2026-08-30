//! `OpDev` command-line entry point.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use opdev_core::{
    AggregateVerdict, EXTENSION_PROTOCOL_VERSION, Gate, Outcome, PROJECT_SCHEMA_VERSION, RuleId,
    embedded_catalog,
};
use opdev_engine::{CheckOptions, CheckReport, evaluate};
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
    /// Validate and aggregate without executing project commands.
    #[arg(long)]
    no_exec: bool,
    /// Report presentation.
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    format: OutputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
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
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Version => {
            let catalog = embedded_catalog().context("could not load the embedded rule catalog")?;
            println!("opdev {}", env!("CARGO_PKG_VERSION"));
            println!("project schema {PROJECT_SCHEMA_VERSION}");
            println!("rule catalog {}", catalog.catalog_version);
            println!("extension protocol {EXTENSION_PROTOCOL_VERSION}");
            Ok(ExitCode::SUCCESS)
        }
        Command::Rules(args) => show_rules(args).map(|()| ExitCode::SUCCESS),
        Command::Init(args) => initialize(&args).map(|()| ExitCode::SUCCESS),
        Command::Check(args) => check_project(&args),
        Command::Doctor(args) => doctor(&args).map(|()| ExitCode::SUCCESS),
        Command::Upgrade(args) => upgrade(&args).map(|()| ExitCode::SUCCESS),
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

fn check_project(args: &CheckArgs) -> Result<ExitCode> {
    let (root, manifest) = load_project(&args.root)?;
    let mut options = if args.ci {
        CheckOptions::pre_merge()
    } else {
        CheckOptions::local()
    };
    options.execute_checks = !args.no_exec;
    let report = evaluate(&root, &manifest, options).context("project evaluation failed")?;
    match args.format {
        OutputFormat::Human => print_human_report(&report),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
    }
    if args.remote {
        eprintln!("note: read-only remote auditing is added in Phase 7; no remote was queried");
    }
    let gate = if args.ci {
        Gate::Integration
    } else {
        Gate::Development
    };
    Ok(if report.gate_passed(gate) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn print_human_report(report: &CheckReport) {
    println!("OpDev report for {}", report.subject);
    for check in &report.checks {
        println!(
            "check {}: {:?} — {}",
            check.id, check.outcome, check.summary
        );
        if let Some(stderr) = &check.stderr {
            for line in stderr.lines().take(8) {
                println!("  {line}");
            }
        }
    }
    let mut counts = [0_u32; 6];
    for result in &report.rules {
        counts[outcome_index(result.outcome)] += 1;
    }
    println!(
        "rules: {} passed, {} failed, {} unverified, {} not_applicable, {} error, {} migration_required",
        counts[0], counts[1], counts[2], counts[3], counts[4], counts[5]
    );
    for gate in &report.gates {
        println!(
            "gate {:?}: {:?} ({} rules, {} checks blocking)",
            gate.gate,
            gate.verdict,
            gate.blocking_rules.len(),
            gate.blocking_checks.len()
        );
        if gate.verdict == AggregateVerdict::Blocked {
            let rules = gate
                .blocking_rules
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            if !rules.is_empty() {
                println!("  rules: {rules}");
            }
            if !gate.blocking_checks.is_empty() {
                println!("  checks: {}", gate.blocking_checks.join(", "));
            }
        }
    }
}

const fn outcome_index(outcome: Outcome) -> usize {
    match outcome {
        Outcome::Passed => 0,
        Outcome::Failed => 1,
        Outcome::Unverified => 2,
        Outcome::NotApplicable => 3,
        Outcome::Error => 4,
        Outcome::MigrationRequired => 5,
    }
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
            vec!["opdev", "check", "--no-exec", "--format", "json"],
            vec!["opdev", "doctor", "--remote"],
            vec!["opdev", "upgrade"],
            vec!["opdev", "version"],
            vec!["opdev", "rules", "--id", "MCD-TRUNK-001"],
        ] {
            assert!(Cli::try_parse_from(arguments).is_ok());
        }
    }
}
