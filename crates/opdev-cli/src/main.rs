//! `OpDev` command-line entry point.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use opdev_ci::{Capability, TemplateContext, adapter_for, infer_gitlab_image, write_new};
use opdev_core::{
    AggregateVerdict, EXTENSION_PROTOCOL_VERSION, Gate, Outcome, PROJECT_SCHEMA_VERSION, RuleId,
    VerificationMethod, embedded_catalog, embedded_profiles, resolve_profile,
};
use opdev_engine::{CheckOptions, CheckReport, evaluate, reaggregate};
use opdev_project::{
    CiProvider, CoverageMode, DeliveryStatus, EVIDENCE_PATH, EvidenceBootstrap, FileChange,
    MANIFEST_PATH, ProjectManifest, RecoveryStrategy, discover, reconcile_agent_files,
    staged_fingerprint,
};
use opdev_release::{
    EvidenceRequest, PackageFormat, PackageInput, PackageRequest, generate_evidence,
    package_release,
};
use opdev_remote::{RemoteAudit, RemoteCapability, audit};
use semver::{Version, VersionReq};
use serde::Deserialize;

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
    /// Generate or inspect a first-class CI configuration.
    Ci(CiArgs),
    /// Upgrade project-owned `OpDev` files explicitly.
    Upgrade(UpgradeArgs),
    /// Show CLI and protocol versions.
    Version,
    /// Inspect the embedded normative rule catalog.
    Rules(RulesArgs),
    /// Inspect exact-version assurance profiles bundled with this release.
    Profiles(ProfilesArgs),
    /// Package already-built artifacts and generate deterministic release evidence.
    Release(ReleaseArgs),
    /// Prepare repository-state binding for reviewable project evidence.
    Evidence(EvidenceArgs),
    /// Verify compatibility between an installed agent plugin and this CLI.
    Plugin(PluginArgs),
}

#[derive(Debug, Args)]
struct PluginArgs {
    #[command(subcommand)]
    command: PluginCommand,
}

#[derive(Debug, Subcommand)]
enum PluginCommand {
    /// Verify a packaged plugin compatibility contract against this CLI.
    Verify(PluginVerifyArgs),
}

#[derive(Debug, Args)]
struct PluginVerifyArgs {
    /// Packaged `opdev-compatibility.json` contract.
    #[arg(long)]
    contract: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginCompatibility {
    schema: u32,
    plugin: PluginIdentity,
    requires: PluginRequirements,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginIdentity {
    name: String,
    version: Version,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginRequirements {
    cli: VersionReq,
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
struct CiArgs {
    #[command(subcommand)]
    command: CiCommand,
}

#[derive(Debug, Subcommand)]
enum CiCommand {
    /// Render a pinned baseline configuration.
    Generate(CiGenerateArgs),
    /// Inspect the initialized project's local CI configuration.
    Inspect(CiInspectArgs),
}

#[derive(Debug, Args)]
struct CiGenerateArgs {
    /// Repository directory.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Provider to render.
    #[arg(long, value_enum)]
    provider: ProviderArg,
    /// Exact `OpDev` release used by CI.
    #[arg(long, default_value = env!("CARGO_PKG_VERSION"))]
    opdev_version: String,
    /// GitLab job image; inferred from exact project toolchain metadata when omitted.
    #[arg(long)]
    image: Option<String>,
    /// Create the provider file; otherwise print it to standard output.
    #[arg(long)]
    write: bool,
}

#[derive(Debug, Args)]
struct CiInspectArgs {
    /// Directory inside the initialized Git repository.
    #[arg(long, default_value = ".")]
    root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ProviderArg {
    Github,
    Gitlab,
}

impl From<ProviderArg> for CiProvider {
    fn from(value: ProviderArg) -> Self {
        match value {
            ProviderArg::Github => Self::Github,
            ProviderArg::Gitlab => Self::Gitlab,
        }
    }
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

#[derive(Debug, Args)]
struct ProfilesArgs {
    /// Show one stable profile name instead of listing bundled profiles.
    #[arg(long, requires = "version")]
    name: Option<String>,
    /// Exact profile version; floating aliases such as `latest` are rejected.
    #[arg(long, requires = "name")]
    version: Option<String>,
}

#[derive(Debug, Args)]
struct ReleaseArgs {
    #[command(subcommand)]
    command: ReleaseCommand,
}

#[derive(Debug, Subcommand)]
enum ReleaseCommand {
    /// Create a deterministic archive from explicit source-to-destination mappings.
    Package(ReleasePackageArgs),
    /// Bind artifacts, source, and an existing `CycloneDX` SBOM by digest.
    Evidence(ReleaseEvidenceArgs),
}

#[derive(Debug, Args)]
struct ReleasePackageArgs {
    /// Archive encoding.
    #[arg(long, value_enum)]
    format: PackageFormatArg,
    /// Regular file or directory mapping in the form SOURCE=DESTINATION; repeat as needed.
    #[arg(long, value_name = "SOURCE=DESTINATION")]
    entry: Vec<String>,
    /// Executable file mapping in the form SOURCE=DESTINATION; repeat as needed.
    #[arg(long, value_name = "SOURCE=DESTINATION")]
    executable_entry: Vec<String>,
    /// New archive path. Existing output is never replaced.
    #[arg(long)]
    output: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum PackageFormatArg {
    TarGz,
    Zip,
}

impl From<PackageFormatArg> for PackageFormat {
    fn from(value: PackageFormatArg) -> Self {
        match value {
            PackageFormatArg::TarGz => Self::TarGz,
            PackageFormatArg::Zip => Self::Zip,
        }
    }
}

#[derive(Debug, Args)]
struct ReleaseEvidenceArgs {
    /// Release artifact; repeat for every artifact in this evidence bundle.
    #[arg(long, required = true)]
    artifact: Vec<PathBuf>,
    /// Existing `CycloneDX` JSON SBOM.
    #[arg(long)]
    sbom: PathBuf,
    /// Exact `CycloneDX` specification version required from the SBOM.
    #[arg(long, default_value = "1.5")]
    sbom_version: String,
    /// Stable source repository URI.
    #[arg(long)]
    source_uri: String,
    /// Exact source revision, normally the Git commit SHA.
    #[arg(long)]
    source_revision: String,
    /// Builder identity URI supplied by the build platform.
    #[arg(long)]
    builder_id: String,
    /// Honest scope or assurance limitation for this artifact and SBOM association.
    #[arg(long)]
    assurance_limitation: Option<String>,
    /// Directory in which evidence files are created without replacement.
    #[arg(long)]
    output: PathBuf,
}

#[derive(Debug, Args)]
struct EvidenceArgs {
    #[command(subcommand)]
    command: EvidenceCommand,
}

#[derive(Debug, Subcommand)]
enum EvidenceCommand {
    /// Print the staged index fingerprint used by change evidence.
    Fingerprint(EvidenceFingerprintArgs),
    /// Generate or apply a fail-closed review questionnaire for a new evidence ledger.
    Bootstrap(EvidenceBootstrapArgs),
}

#[derive(Debug, Args)]
struct EvidenceFingerprintArgs {
    /// Directory inside the initialized Git repository.
    #[arg(long, default_value = ".")]
    root: PathBuf,
}

#[derive(Debug, Args)]
struct EvidenceBootstrapArgs {
    /// Directory inside the initialized Git repository.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Completed questionnaire to validate and expand; omit to print a new questionnaire.
    #[arg(long)]
    answers: Option<PathBuf>,
    /// Create `.opdev/evidence.yaml`; otherwise print the candidate ledger.
    #[arg(long, requires = "answers")]
    write: bool,
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
        Command::Profiles(args) => show_profiles(args).map(|()| ExitCode::SUCCESS),
        Command::Release(args) => release_command(&args).map(|()| ExitCode::SUCCESS),
        Command::Evidence(args) => evidence_command(&args).map(|()| ExitCode::SUCCESS),
        Command::Plugin(args) => plugin_command(&args),
        Command::Init(args) => initialize(&args).map(|()| ExitCode::SUCCESS),
        Command::Check(args) => check_project(&args),
        Command::Doctor(args) => doctor(&args).map(|()| ExitCode::SUCCESS),
        Command::Ci(args) => ci_command(&args).map(|()| ExitCode::SUCCESS),
        Command::Upgrade(args) => upgrade(&args).map(|()| ExitCode::SUCCESS),
    }
}

fn plugin_command(args: &PluginArgs) -> Result<ExitCode> {
    match &args.command {
        PluginCommand::Verify(args) => verify_plugin_compatibility(args),
    }
}

fn verify_plugin_compatibility(args: &PluginVerifyArgs) -> Result<ExitCode> {
    let source = std::fs::read_to_string(&args.contract).with_context(|| {
        format!(
            "could not read plugin compatibility contract {}",
            args.contract.display()
        )
    })?;
    let contract: PluginCompatibility = serde_json::from_str(&source).with_context(|| {
        format!(
            "invalid plugin compatibility contract {}",
            args.contract.display()
        )
    })?;
    if contract.schema != 1 {
        bail!(
            "unsupported plugin compatibility schema {}; this CLI supports schema 1",
            contract.schema
        );
    }
    if contract.plugin.name != "opdev" {
        bail!(
            "compatibility contract names plugin `{}`; expected `opdev`",
            contract.plugin.name
        );
    }

    let cli_version = Version::parse(env!("CARGO_PKG_VERSION"))
        .context("the compiled CLI version is not valid SemVer")?;
    if contract.requires.cli.matches(&cli_version) {
        println!(
            "opdev plugin {} is compatible with CLI {} ({})",
            contract.plugin.version, cli_version, contract.requires.cli
        );
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!(
            "opdev plugin {} requires CLI {}; installed CLI is {}",
            contract.plugin.version, contract.requires.cli, cli_version
        );
        Ok(ExitCode::from(1))
    }
}

fn evidence_command(args: &EvidenceArgs) -> Result<()> {
    match &args.command {
        EvidenceCommand::Fingerprint(args) => {
            let (root, _) = load_project(&args.root)?;
            println!("{}", staged_fingerprint(&root)?);
        }
        EvidenceCommand::Bootstrap(args) => bootstrap_evidence(args)?,
    }
    Ok(())
}

fn bootstrap_evidence(args: &EvidenceBootstrapArgs) -> Result<()> {
    let (root, manifest) = load_project(&args.root)?;
    let ledger_path = root.join(EVIDENCE_PATH);
    if ledger_path.exists() {
        bail!(
            "{} already exists; bootstrap is intentionally create-new only",
            ledger_path.display()
        );
    }

    let fingerprint = staged_fingerprint(&root)?;
    let mut report = evaluate(
        &root,
        &manifest,
        CheckOptions {
            execute_checks: false,
            ..CheckOptions::pre_merge()
        },
    )
    .context("project evaluation failed")?;
    apply_local_ci(&root, &manifest, &mut report)?;
    let catalog = embedded_catalog().context("could not load the embedded rule catalog")?;
    let (project_rules, change_rules) = evidence_candidates(&catalog, &report);

    if let Some(path) = &args.answers {
        let answers = EvidenceBootstrap::load(path)?;
        answers.validate_candidates(&project_rules, &change_rules, &fingerprint)?;
        let ledger = answers.to_ledger(&catalog)?;
        if args.write {
            let path = ledger.write_new(&root, &catalog)?;
            println!("created {}", path.display());
        } else {
            print!("{}", ledger.to_yaml()?);
            eprintln!(
                "candidate only: review this expansion, then repeat with --write to create the ledger"
            );
        }
    } else {
        let questionnaire = EvidenceBootstrap::new(
            fingerprint,
            project_rules.iter().cloned(),
            change_rules.iter().cloned(),
        );
        print!("{}", questionnaire.to_review_yaml(&catalog)?);
        eprintln!(
            "all decisions are review_required; add shared evidence and explicitly review each outcome"
        );
    }
    Ok(())
}

fn evidence_candidates(
    catalog: &opdev_core::RuleCatalog,
    report: &CheckReport,
) -> (Vec<String>, Vec<String>) {
    let mut project = Vec::new();
    let mut change = Vec::new();
    for rule in &catalog.rules {
        let unverified = report
            .rules
            .iter()
            .find(|result| result.rule_id == rule.id)
            .is_some_and(|result| result.outcome == Outcome::Unverified);
        let reviewable = rule.verification.contains(&VerificationMethod::Evidence)
            || rule.verification.contains(&VerificationMethod::Agent);
        if !unverified || !reviewable {
            continue;
        }
        let destination = if change_scoped_rule(rule.id.as_str()) {
            &mut change
        } else {
            &mut project
        };
        destination.push(rule.id.as_str().to_owned());
    }
    (project, change)
}

fn change_scoped_rule(rule_id: &str) -> bool {
    matches!(
        rule_id,
        "OPDEV-WORK-001"
            | "OPDEV-DESIGN-001"
            | "MCD-TRUNK-002"
            | "MCD-TRUNK-003"
            | "MCD-FLOW-001"
            | "MCD-COMPAT-001"
            | "OPDEV-TEST-002"
            | "OPDEV-TEST-003"
            | "OPDEV-TEST-004"
            | "OPDEV-AI-001"
            | "OPDEV-LEARN-001"
    )
}

fn release_command(args: &ReleaseArgs) -> Result<()> {
    match &args.command {
        ReleaseCommand::Package(args) => {
            let mut inputs = args
                .entry
                .iter()
                .map(|entry| parse_package_input(entry, false))
                .collect::<Result<Vec<_>>>()?;
            inputs.extend(
                args.executable_entry
                    .iter()
                    .map(|entry| parse_package_input(entry, true))
                    .collect::<Result<Vec<_>>>()?,
            );
            package_release(&PackageRequest {
                inputs,
                format: args.format.into(),
                output: args.output.clone(),
            })?;
            println!("created {}", args.output.display());
        }
        ReleaseCommand::Evidence(args) => {
            let outputs = generate_evidence(&EvidenceRequest {
                artifacts: args.artifact.clone(),
                sbom: args.sbom.clone(),
                sbom_version: args.sbom_version.clone(),
                source_uri: args.source_uri.clone(),
                source_revision: args.source_revision.clone(),
                builder_id: args.builder_id.clone(),
                assurance_limitation: args.assurance_limitation.clone(),
                output_directory: args.output.clone(),
            })?;
            println!("created {}", outputs.checksums.display());
            println!("created {}", outputs.manifest.display());
            println!("created {}", outputs.provenance.display());
            println!(
                "Evidence is digest-bound but does not by itself establish signing, trusted-builder provenance, or a SLSA Build level."
            );
        }
    }
    Ok(())
}

fn parse_package_input(value: &str, executable: bool) -> Result<PackageInput> {
    let (source, destination) = value
        .split_once('=')
        .filter(|(source, destination)| !source.is_empty() && !destination.is_empty())
        .ok_or_else(|| anyhow::anyhow!("package entry `{value}` must use SOURCE=DESTINATION"))?;
    Ok(PackageInput {
        source: PathBuf::from(source),
        destination: destination.to_owned(),
        executable,
    })
}

fn show_profiles(args: ProfilesArgs) -> Result<()> {
    if let (Some(name), Some(version)) = (args.name, args.version) {
        let profile = resolve_profile(&name, &version, None)?;
        println!("{}", serde_json::to_string_pretty(&profile)?);
    } else {
        for profile in embedded_profiles()? {
            println!(
                "{}@{}\t{:?}\t{}",
                profile.name, profile.version, profile.status, profile.title
            );
        }
    }
    Ok(())
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

fn ci_command(args: &CiArgs) -> Result<()> {
    match &args.command {
        CiCommand::Generate(args) => generate_ci(args),
        CiCommand::Inspect(args) => inspect_ci(args),
    }
}

fn generate_ci(args: &CiGenerateArgs) -> Result<()> {
    let discovery = discover(&args.root).context("could not inspect the repository")?;
    let provider: CiProvider = args.provider.into();
    let adapter = adapter_for(provider)?;
    let job_image = match (provider, &args.image) {
        (CiProvider::Gitlab, Some(image)) => Some(image.clone()),
        (CiProvider::Gitlab, None) => Some(infer_gitlab_image(&discovery.root)?),
        (CiProvider::Github, Some(_)) => {
            bail!("`--image` is supported only when generating GitLab CI")
        }
        _ => None,
    };
    let context = TemplateContext {
        opdev_version: args.opdev_version.clone(),
        trunk: discovery.manifest.project.trunk,
        job_image,
    };
    if args.write {
        let path = write_new(adapter, &discovery.root, &context)?;
        println!("created {}", path.display());
    } else {
        print!("{}", adapter.render(&context)?);
    }
    Ok(())
}

fn inspect_ci(args: &CiInspectArgs) -> Result<()> {
    let (root, manifest) = load_project(&args.root)?;
    let adapter = adapter_for(manifest.project.ci.provider)?;
    let inspection = adapter.inspect(&root)?;
    print_capability("configuration", &inspection.configuration);
    print_capability("pre_merge", &inspection.pre_merge);
    print_capability("post_merge", &inspection.post_merge);
    print_capability("integrity", &inspection.integrity);
    Ok(())
}

fn print_capability(name: &str, capability: &Capability) {
    println!("{name}: {:?}", capability.outcome);
    if let Some(diagnostic) = &capability.diagnostic {
        println!("  {diagnostic}");
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
    let mut report = evaluate(&root, &manifest, options).context("project evaluation failed")?;
    if args.ci {
        apply_local_ci(&root, &manifest, &mut report)?;
    }
    if args.remote {
        apply_remote_audit(&manifest, &mut report)?;
    }
    match args.format {
        OutputFormat::Human => print_human_report(&report),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
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

fn apply_local_ci(root: &Path, manifest: &ProjectManifest, report: &mut CheckReport) -> Result<()> {
    let Ok(adapter) = adapter_for(manifest.project.ci.provider) else {
        let outcome = if manifest.project.ci.provider == CiProvider::Unconfigured {
            Outcome::MigrationRequired
        } else {
            Outcome::Unverified
        };
        let capability = Capability {
            outcome,
            evidence: Vec::new(),
            diagnostic: Some(format!(
                "No first-class local adapter is available for {:?}",
                manifest.project.ci.provider
            )),
        };
        apply_capability(report, "MCD-CI-001", &capability);
        apply_capability(report, "MCD-TEST-001", &capability);
        apply_capability(report, "MCD-TEST-002", &capability);
        reaggregate(report)?;
        return Ok(());
    };
    let inspection = adapter.inspect(root)?;
    apply_capability(report, "MCD-CI-001", &inspection.configuration);
    if inspection.integrity.outcome != Outcome::Passed {
        apply_capability(report, "MCD-CI-001", &inspection.integrity);
    }
    apply_capability(report, "MCD-TEST-001", &inspection.pre_merge);
    apply_capability(report, "MCD-TEST-002", &inspection.post_merge);
    reaggregate(report)?;
    Ok(())
}

fn apply_capability(report: &mut CheckReport, rule_id: &str, capability: &Capability) {
    if let Some(result) = report
        .rules
        .iter_mut()
        .find(|result| result.rule_id.as_str() == rule_id)
    {
        result.outcome = capability.outcome;
        result.verifier = opdev_core::VerificationSource::Ci;
        result.evidence.clone_from(&capability.evidence);
        result.diagnostic.clone_from(&capability.diagnostic);
    }
}

fn apply_remote_audit(manifest: &ProjectManifest, report: &mut CheckReport) -> Result<()> {
    let audit = audit(manifest).context("read-only remote audit could not start")?;
    apply_remote_capability(report, "MCD-CI-001", &audit.ci);
    apply_remote_capability(report, "MCD-TRUNK-001", &audit.trunk);
    apply_remote_capability(report, "MCD-TEST-002", &audit.trunk_pipeline);

    let mut flow = audit.trunk_pipeline.clone();
    if flow.outcome == Outcome::Passed {
        flow.outcome = Outcome::NotApplicable;
        flow.diagnostic = None;
        flow.evidence.push(opdev_core::Evidence {
            kind: "applicability".into(),
            summary:
                "The latest trunk pipeline is green, so the red-trunk stop rule does not apply"
                    .into(),
            location: None,
        });
    }
    apply_remote_capability(report, "MCD-FLOW-001", &flow);

    let mut lifecycle_evidence = audit.branch_lifecycle.evidence.clone();
    lifecycle_evidence.extend(audit.trunk_protection.evidence.clone());
    let lifecycle = RemoteCapability {
        outcome: Outcome::Unverified,
        evidence: lifecycle_evidence,
        diagnostic: Some(
            "Provider settings do not prove branch origin, age, daily integration, or deletion for every branch"
                .into(),
        ),
    };
    apply_remote_capability(report, "MCD-TRUNK-002", &lifecycle);
    reaggregate(report)?;
    Ok(())
}

fn apply_remote_capability(report: &mut CheckReport, rule_id: &str, capability: &RemoteCapability) {
    if let Some(result) = report
        .rules
        .iter_mut()
        .find(|result| result.rule_id.as_str() == rule_id)
    {
        if !remote_capability_should_replace(result.outcome, capability.outcome) {
            return;
        }
        result.outcome = capability.outcome;
        result.verifier = opdev_core::VerificationSource::Remote;
        result.evidence.clone_from(&capability.evidence);
        result.diagnostic.clone_from(&capability.diagnostic);
    }
}

fn remote_capability_should_replace(current: Outcome, remote: Outcome) -> bool {
    remote != Outcome::Unverified || !current.satisfies_required_rule()
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
        let audit = audit(&manifest).context("read-only remote audit could not start")?;
        print_remote_audit(&audit);
    }
    Ok(())
}

fn print_remote_audit(audit: &RemoteAudit) {
    println!("Remote: {}", audit.repository);
    for (name, capability) in [
        ("ci", &audit.ci),
        ("trunk", &audit.trunk),
        ("trunk_protection", &audit.trunk_protection),
        ("trunk_pipeline", &audit.trunk_pipeline),
        ("branch_lifecycle", &audit.branch_lifecycle),
    ] {
        println!("- {name}: {:?}", capability.outcome);
        if let Some(diagnostic) = &capability.diagnostic {
            println!("  {diagnostic}");
        }
    }
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
            vec!["opdev", "ci", "generate", "--provider", "gitlab"],
            vec![
                "opdev",
                "ci",
                "generate",
                "--provider",
                "gitlab",
                "--image",
                "example.test/toolchain:1",
            ],
            vec!["opdev", "ci", "inspect"],
            vec!["opdev", "upgrade"],
            vec!["opdev", "version"],
            vec![
                "opdev",
                "plugin",
                "verify",
                "--contract",
                "opdev-compatibility.json",
            ],
            vec!["opdev", "rules", "--id", "MCD-TRUNK-001"],
            vec!["opdev", "profiles"],
            vec![
                "opdev",
                "profiles",
                "--name",
                "opdev-core",
                "--version",
                "1",
            ],
            vec!["opdev", "evidence", "fingerprint"],
            vec!["opdev", "evidence", "bootstrap"],
            vec![
                "opdev",
                "evidence",
                "bootstrap",
                "--answers",
                "review.yaml",
                "--write",
            ],
            vec![
                "opdev",
                "release",
                "package",
                "--format",
                "tar-gz",
                "--executable-entry",
                "target/release/opdev=opdev",
                "--entry",
                "LICENSE=LICENSE",
                "--output",
                "opdev.tar.gz",
            ],
            vec![
                "opdev",
                "release",
                "evidence",
                "--artifact",
                "opdev.tar.gz",
                "--sbom",
                "opdev.cdx.json",
                "--source-uri",
                "https://example.test/opdev",
                "--source-revision",
                "0123456789abcdef",
                "--builder-id",
                "https://example.test/builders/1",
                "--output",
                "release",
            ],
        ] {
            assert!(Cli::try_parse_from(arguments).is_ok());
        }
    }

    #[test]
    fn cli_and_plugin_versions_stay_in_sync() -> Result<()> {
        let expected = env!("CARGO_PKG_VERSION");
        let claude_marketplace: serde_json::Value =
            serde_json::from_str(include_str!("../../../.claude-plugin/marketplace.json"))?;
        let claude_plugin: serde_json::Value = serde_json::from_str(include_str!(
            "../../../plugins/opdev/.claude-plugin/plugin.json"
        ))?;
        let codex_plugin: serde_json::Value = serde_json::from_str(include_str!(
            "../../../plugins/opdev/.codex-plugin/plugin.json"
        ))?;
        let compatibility: PluginCompatibility = serde_json::from_str(include_str!(
            "../../../plugins/opdev/opdev-compatibility.json"
        ))?;

        assert_eq!(claude_marketplace["version"], expected);
        assert_eq!(claude_marketplace["plugins"][0]["version"], expected);
        assert_eq!(claude_plugin["version"], expected);
        assert_eq!(codex_plugin["version"], expected);
        assert_eq!(compatibility.plugin.version.to_string(), expected);
        assert!(
            compatibility
                .requires
                .cli
                .matches(&Version::parse(expected)?)
        );
        Ok(())
    }

    #[test]
    fn plugin_compatibility_is_fail_closed() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let contract = directory.path().join("compatibility.json");
        std::fs::write(
            &contract,
            r#"{"schema":1,"plugin":{"name":"opdev","version":"99.0.0"},"requires":{"cli":">=99.0.0, <100.0.0"}}"#,
        )?;

        assert_eq!(
            verify_plugin_compatibility(&PluginVerifyArgs {
                contract: contract.clone()
            })?,
            ExitCode::from(1)
        );

        std::fs::write(
            &contract,
            r#"{"schema":2,"plugin":{"name":"opdev","version":"0.1.1"},"requires":{"cli":">=0.1.1, <0.2.0"}}"#,
        )?;
        assert!(
            verify_plugin_compatibility(&PluginVerifyArgs { contract }).is_err(),
            "unknown compatibility schemas must not activate"
        );
        Ok(())
    }

    #[test]
    fn inconclusive_remote_audit_does_not_erase_satisfying_evidence() {
        assert!(!remote_capability_should_replace(
            Outcome::Passed,
            Outcome::Unverified
        ));
        assert!(!remote_capability_should_replace(
            Outcome::NotApplicable,
            Outcome::Unverified
        ));
        assert!(remote_capability_should_replace(
            Outcome::Passed,
            Outcome::Failed
        ));
        assert!(remote_capability_should_replace(
            Outcome::MigrationRequired,
            Outcome::Unverified
        ));
    }

    #[test]
    fn node_and_go_bootstrap_inputs_are_smaller_without_changing_rule_outcomes()
    -> Result<(), Box<dyn std::error::Error>> {
        for (file, contents) in [
            (
                "package.json",
                r#"{"name":"bootstrap-node","scripts":{"test":"node --test"}}"#,
            ),
            ("go.mod", "module example.test/bootstrap-go\n\ngo 1.24\n"),
        ] {
            let directory = tempfile::tempdir()?;
            let root = directory.path();
            assert!(
                std::process::Command::new("git")
                    .arg("init")
                    .arg(root)
                    .status()?
                    .success()
            );
            std::fs::write(root.join(file), contents)?;
            std::fs::write(
                root.join("OPDEV_ADOPTION.md"),
                "Reviewed project policy, applicability, change scope, tests, and integration behavior.\n",
            )?;
            let discovery = discover(root)?;
            discovery.manifest.write_new(&root.join(MANIFEST_PATH))?;
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(root)
                    .args(["add", "."])
                    .status()?
                    .success()
            );

            let fingerprint = staged_fingerprint(root)?;
            let mut report = evaluate(
                root,
                &discovery.manifest,
                CheckOptions {
                    execute_checks: false,
                    ..CheckOptions::pre_merge()
                },
            )?;
            apply_local_ci(root, &discovery.manifest, &mut report)?;
            let catalog = embedded_catalog()?;
            let (project_rules, change_rules) = evidence_candidates(&catalog, &report);
            assert!(!project_rules.is_empty());
            assert!(!change_rules.is_empty());

            let mut review = EvidenceBootstrap::new(
                fingerprint.clone(),
                project_rules.clone(),
                change_rules.clone(),
            );
            for decision in review.project.decisions.values_mut() {
                *decision = opdev_project::ReviewDecision::Passed;
            }
            for decision in review.change.decisions.values_mut() {
                *decision = opdev_project::ReviewDecision::Passed;
            }
            let shared = opdev_core::Evidence {
                kind: "review".into(),
                summary: "The adoption review records the facts supporting these decisions.".into(),
                location: Some("OPDEV_ADOPTION.md".into()),
            };
            review.project.evidence.push(shared.clone());
            review.change.evidence.push(shared);
            review.change.work = "OPDEV-15 greenfield adoption review".into();
            review.validate_candidates(&project_rules, &change_rules, &fingerprint)?;
            let review_yaml = review.to_yaml()?;
            let ledger = review.to_ledger(&catalog)?;
            let ledger_yaml = ledger.to_yaml()?;
            assert!(review_yaml.lines().count() * 2 < ledger_yaml.lines().count());
            ledger.write_new(root, &catalog)?;

            let mut verified = evaluate(
                root,
                &discovery.manifest,
                CheckOptions {
                    execute_checks: false,
                    ..CheckOptions::pre_merge()
                },
            )?;
            apply_local_ci(root, &discovery.manifest, &mut verified)?;
            for rule_id in project_rules.iter().chain(&change_rules) {
                assert_eq!(
                    verified
                        .rules
                        .iter()
                        .find(|result| result.rule_id.as_str() == rule_id)
                        .map(|result| result.outcome),
                    Some(Outcome::Passed),
                    "{file} did not preserve the accepted outcome for {rule_id}"
                );
            }
        }
        Ok(())
    }
}
