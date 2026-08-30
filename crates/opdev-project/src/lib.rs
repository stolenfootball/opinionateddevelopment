//! Project contracts, repository discovery, and initialization for `OpDev`.

#![forbid(unsafe_code)]

mod bootstrap;
mod discovery;
mod manifest;

pub use bootstrap::{BootstrapError, FileChange, ManagedFile, reconcile_agent_files};
pub use discovery::{Discovery, DiscoveryError, discover};
pub use manifest::{
    Artifact, Assurance, AuthorityKind, AuthorityRef, ChangeTests, CiConfig, CiProvider,
    CommandSpec, Context, Coverage, CoverageMode, Delivery, DeliveryMode, DeliveryStatus,
    Environment, EscapedDefectRegressions, ExtensionCheck, ExtensionStage, Extensions, FlakePolicy,
    ManifestError, Operations, Profile, Project, ProjectKind, ProjectManifest, Quality,
    QualityRisk, Recovery, RecoveryStrategy, TestStage, TestSuite, Testing,
};

/// Repository-relative location of an initialized project contract.
pub const MANIFEST_PATH: &str = ".opdev/project.yaml";
