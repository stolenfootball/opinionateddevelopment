//! Deterministic checksum, SBOM-association, and provenance evidence generation.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};

use flate2::{Compression, GzBuilder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zip::write::SimpleFileOptions;

/// Release evidence bundle format emitted by this release.
pub const RELEASE_EVIDENCE_VERSION: u32 = 1;
/// SLSA provenance predicate used by the emitted in-toto statement.
pub const SLSA_PROVENANCE_PREDICATE: &str = "https://slsa.dev/provenance/v1";
/// OpDev-specific build type used by the emitted predicate.
pub const OPDEV_BUILD_TYPE: &str = "https://opdev.dev/buildtypes/release/v1";

/// Failure while generating deterministic release evidence.
#[derive(Debug, Error)]
pub enum ReleaseEvidenceError {
    /// An input file could not be read.
    #[error("could not read release input `{path}`: {source}")]
    Read {
        /// Input path.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// An output file or directory could not be created.
    #[error("could not write release evidence `{path}`: {source}")]
    Write {
        /// Output path.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// The SBOM is not valid JSON.
    #[error("SBOM `{path}` is not valid JSON: {source}")]
    SbomJson {
        /// SBOM path.
        path: PathBuf,
        /// JSON parse failure.
        source: serde_json::Error,
    },
    /// The SBOM is not the required `CycloneDX` version.
    #[error(
        "SBOM `{path}` must be CycloneDX {expected}; found bomFormat={format:?}, specVersion={version:?}"
    )]
    SbomFormat {
        /// SBOM path.
        path: PathBuf,
        /// Required exact specification version.
        expected: String,
        /// Actual or missing format.
        format: Option<String>,
        /// Actual or missing version.
        version: Option<String>,
    },
    /// Two inputs would have the same release filename.
    #[error("release evidence inputs contain duplicate filename `{0}`")]
    DuplicateName(String),
    /// A path has no portable filename.
    #[error("release input `{0}` has no portable filename")]
    MissingName(PathBuf),
    /// A required request value was empty.
    #[error("release evidence field `{0}` must not be empty")]
    EmptyField(&'static str),
    /// JSON serialization failed.
    #[error("release evidence could not be serialized: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Failure while creating a deterministic release package.
#[derive(Debug, Error)]
pub enum PackageError {
    /// A package input could not be inspected or read.
    #[error("could not read package input `{path}`: {source}")]
    Read {
        /// Input path.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// A package output could not be created or completed.
    #[error("could not write package `{path}`: {source}")]
    Write {
        /// Output path.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// ZIP encoding failed.
    #[error("could not encode ZIP package `{path}`: {source}")]
    Zip {
        /// Output path.
        path: PathBuf,
        /// ZIP format failure.
        source: zip::result::ZipError,
    },
    /// An input is neither a regular file nor a directory.
    #[error("package input `{0}` must be a regular file or directory; symbolic links are refused")]
    UnsupportedInput(PathBuf),
    /// An archive destination is empty, absolute, or escapes the archive root.
    #[error("package destination `{0}` must be a portable relative path")]
    InvalidDestination(String),
    /// Two inputs resolve to the same archive path.
    #[error("package inputs contain duplicate destination `{0}`")]
    DuplicateDestination(String),
    /// No regular files were selected for the package.
    #[error("release package must contain at least one regular file")]
    Empty,
}

/// Deterministic archive encoding used for a release package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageFormat {
    /// Gzip-compressed POSIX tar archive.
    TarGz,
    /// ZIP archive with stored file entries.
    Zip,
}

/// One file or directory tree mapped into a release package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInput {
    /// Existing regular file or directory to package.
    pub source: PathBuf,
    /// Relative destination path inside the archive.
    pub destination: String,
    /// Whether every file selected by this input should be executable.
    pub executable: bool,
}

/// Inputs required to create one deterministic release package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageRequest {
    /// Files and directory trees included in the package.
    pub inputs: Vec<PackageInput>,
    /// Archive encoding.
    pub format: PackageFormat,
    /// New archive path. Existing files are never replaced.
    pub output: PathBuf,
}

#[derive(Debug)]
struct ResolvedPackageInput {
    source: PathBuf,
    destination: String,
    mode: u32,
}

/// Creates a byte-reproducible release archive without replacing existing
/// output.
///
/// Entries are sorted by portable archive path. Metadata uses zero timestamps,
/// numeric owner and group zero for tar, and fixed Unix permissions. Symbolic
/// links and path traversal are refused.
///
/// # Errors
///
/// Returns [`PackageError`] for invalid mappings, unsupported inputs,
/// duplicate destinations, or filesystem and encoding failures.
pub fn package_release(request: &PackageRequest) -> Result<(), PackageError> {
    let entries = resolve_package_inputs(&request.inputs)?;
    if entries.is_empty() {
        return Err(PackageError::Empty);
    }
    if let Some(parent) = request
        .output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| PackageError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    match request.format {
        PackageFormat::TarGz => write_tar_gz(&entries, &request.output),
        PackageFormat::Zip => write_zip(&entries, &request.output),
    }
}

fn resolve_package_inputs(
    inputs: &[PackageInput],
) -> Result<Vec<ResolvedPackageInput>, PackageError> {
    let mut resolved = Vec::new();
    for input in inputs {
        let destination = normalize_destination(&input.destination)?;
        collect_package_input(input, &input.source, &destination, &mut resolved)?;
    }
    resolved.sort_by(|left, right| left.destination.cmp(&right.destination));
    let mut destinations = HashSet::new();
    for entry in &resolved {
        if !destinations.insert(entry.destination.clone()) {
            return Err(PackageError::DuplicateDestination(
                entry.destination.clone(),
            ));
        }
    }
    Ok(resolved)
}

fn collect_package_input(
    input: &PackageInput,
    source: &Path,
    destination: &str,
    resolved: &mut Vec<ResolvedPackageInput>,
) -> Result<(), PackageError> {
    let metadata = fs::symlink_metadata(source).map_err(|source_error| PackageError::Read {
        path: source.to_path_buf(),
        source: source_error,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(PackageError::UnsupportedInput(source.to_path_buf()));
    }
    if metadata.is_file() {
        let executable = input.executable
            || Path::new(destination)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("sh"));
        resolved.push(ResolvedPackageInput {
            source: source.to_path_buf(),
            destination: destination.to_owned(),
            mode: if executable { 0o755 } else { 0o644 },
        });
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(PackageError::UnsupportedInput(source.to_path_buf()));
    }

    let mut children = fs::read_dir(source)
        .map_err(|source_error| PackageError::Read {
            path: source.to_path_buf(),
            source: source_error,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source_error| PackageError::Read {
            path: source.to_path_buf(),
            source: source_error,
        })?;
    children.sort_by_key(std::fs::DirEntry::file_name);
    for child in children {
        let name = child
            .file_name()
            .into_string()
            .map_err(|_| PackageError::UnsupportedInput(child.path()))?;
        let child_destination = format!("{destination}/{name}");
        collect_package_input(input, &child.path(), &child_destination, resolved)?;
    }
    Ok(())
}

fn normalize_destination(destination: &str) -> Result<String, PackageError> {
    let portable = destination.replace('\\', "/");
    let path = Path::new(&portable);
    if portable.is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(PackageError::InvalidDestination(destination.to_owned()));
    }
    let components = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    if components.is_empty() || components.iter().any(|component| component.is_empty()) {
        return Err(PackageError::InvalidDestination(destination.to_owned()));
    }
    Ok(components.join("/"))
}

fn create_new_output(path: &Path) -> Result<fs::File, PackageError> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| PackageError::Write {
            path: path.to_path_buf(),
            source,
        })
}

fn write_tar_gz(entries: &[ResolvedPackageInput], output: &Path) -> Result<(), PackageError> {
    let output_file = create_new_output(output)?;
    let encoder = GzBuilder::new()
        .mtime(0)
        .write(output_file, Compression::best());
    let mut archive = tar::Builder::new(encoder);
    archive.mode(tar::HeaderMode::Deterministic);
    for entry in entries {
        let mut source =
            fs::File::open(&entry.source).map_err(|source_error| PackageError::Read {
                path: entry.source.clone(),
                source: source_error,
            })?;
        let size = source
            .metadata()
            .map_err(|source_error| PackageError::Read {
                path: entry.source.clone(),
                source: source_error,
            })?
            .len();
        let mut header = tar::Header::new_gnu();
        header.set_size(size);
        header.set_mode(entry.mode);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_cksum();
        archive
            .append_data(&mut header, &entry.destination, &mut source)
            .map_err(|source_error| PackageError::Write {
                path: output.to_path_buf(),
                source: source_error,
            })?;
    }
    archive.finish().map_err(|source| PackageError::Write {
        path: output.to_path_buf(),
        source,
    })?;
    let encoder = archive.into_inner().map_err(|source| PackageError::Write {
        path: output.to_path_buf(),
        source,
    })?;
    encoder.finish().map_err(|source| PackageError::Write {
        path: output.to_path_buf(),
        source,
    })?;
    Ok(())
}

fn write_zip(entries: &[ResolvedPackageInput], output: &Path) -> Result<(), PackageError> {
    let output_file = create_new_output(output)?;
    let mut archive = zip::ZipWriter::new(output_file);
    for entry in entries {
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .last_modified_time(zip::DateTime::default())
            .unix_permissions(entry.mode);
        archive
            .start_file(&entry.destination, options)
            .map_err(|source| PackageError::Zip {
                path: output.to_path_buf(),
                source,
            })?;
        let mut source =
            fs::File::open(&entry.source).map_err(|source_error| PackageError::Read {
                path: entry.source.clone(),
                source: source_error,
            })?;
        std::io::copy(&mut source, &mut archive).map_err(|source| PackageError::Write {
            path: output.to_path_buf(),
            source,
        })?;
    }
    archive.finish().map_err(|source| PackageError::Zip {
        path: output.to_path_buf(),
        source,
    })?;
    Ok(())
}

/// Inputs required to generate a release evidence bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRequest {
    /// Release artifacts to identify.
    pub artifacts: Vec<PathBuf>,
    /// Existing `CycloneDX` JSON SBOM associated with the artifacts.
    pub sbom: PathBuf,
    /// Exact `CycloneDX` version required from the SBOM.
    pub sbom_version: String,
    /// Stable source repository URI.
    pub source_uri: String,
    /// Exact source revision, normally a Git commit.
    pub source_revision: String,
    /// Builder identity URI supplied by the build environment.
    pub builder_id: String,
    /// Optional caller-supplied limitation that replaces the conservative default.
    pub assurance_limitation: Option<String>,
    /// New or existing directory in which new evidence files are created.
    pub output_directory: PathBuf,
}

/// Digest identity for a release file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileDigest {
    /// Portable release filename.
    pub name: String,
    /// Lowercase SHA-256 digest.
    pub sha256: String,
}

/// SBOM identity and format metadata.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SbomEvidence {
    /// Portable SBOM filename.
    pub name: String,
    /// Lowercase SHA-256 digest.
    pub sha256: String,
    /// SBOM format.
    pub format: String,
    /// Exact format version.
    pub version: String,
}

/// Deterministic manifest binding release artifacts to source and SBOM identity.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    /// Evidence bundle schema version.
    pub schema: u32,
    /// Exact source repository URI.
    pub source_uri: String,
    /// Exact source revision.
    pub source_revision: String,
    /// Self-asserted or build-platform builder identity.
    pub builder_id: String,
    /// Digest identities of release artifacts.
    pub artifacts: Vec<FileDigest>,
    /// SBOM digest and format identity.
    pub sbom: SbomEvidence,
    /// Non-conformance interpretation required for honest use.
    pub assurance_limitation: String,
}

/// Files created by release evidence generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceOutputs {
    /// Checksum list path.
    pub checksums: PathBuf,
    /// Release manifest path.
    pub manifest: PathBuf,
    /// SLSA-compatible in-toto statement path.
    pub provenance: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InTotoStatement<'a> {
    #[serde(rename = "_type")]
    statement_type: &'static str,
    subject: &'a [ProvenanceSubject],
    predicate_type: &'static str,
    predicate: ProvenancePredicate<'a>,
}

#[derive(Debug, Serialize)]
struct ProvenanceSubject {
    name: String,
    digest: BTreeMap<&'static str, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProvenancePredicate<'a> {
    build_definition: BuildDefinition<'a>,
    run_details: RunDetails<'a>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildDefinition<'a> {
    build_type: &'static str,
    external_parameters: ExternalParameters<'a>,
    internal_parameters: BTreeMap<String, String>,
    resolved_dependencies: Vec<ResourceDescriptor<'a>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExternalParameters<'a> {
    source_uri: &'a str,
    source_revision: &'a str,
    sbom: &'a SbomEvidence,
}

#[derive(Debug, Serialize)]
struct ResourceDescriptor<'a> {
    uri: &'a str,
    digest: BTreeMap<&'static str, &'a str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunDetails<'a> {
    builder: Builder<'a>,
    metadata: BTreeMap<String, String>,
    byproducts: Vec<ResourceDescriptor<'a>>,
}

#[derive(Debug, Serialize)]
struct Builder<'a> {
    id: &'a str,
}

/// Generates checksum, manifest, and SLSA-compatible provenance files without
/// replacing existing evidence.
///
/// The output intentionally does not claim a SLSA Build level. Builder trust,
/// signing, isolation, and verification are properties of the calling build
/// platform and distribution process.
///
/// # Errors
///
/// Returns [`ReleaseEvidenceError`] for invalid input, format mismatches,
/// duplicate filenames, filesystem failures, or serialization failures.
pub fn generate_evidence(
    request: &EvidenceRequest,
) -> Result<EvidenceOutputs, ReleaseEvidenceError> {
    let manifest = prepare_manifest(request)?;
    fs::create_dir_all(&request.output_directory).map_err(|source| {
        ReleaseEvidenceError::Write {
            path: request.output_directory.clone(),
            source,
        }
    })?;
    write_evidence(&manifest, &request.output_directory)
}

fn prepare_manifest(request: &EvidenceRequest) -> Result<ReleaseManifest, ReleaseEvidenceError> {
    validate_non_empty("source_uri", &request.source_uri)?;
    validate_non_empty("source_revision", &request.source_revision)?;
    validate_non_empty("builder_id", &request.builder_id)?;
    validate_non_empty("sbom_version", &request.sbom_version)?;
    if let Some(limitation) = &request.assurance_limitation {
        validate_non_empty("assurance_limitation", limitation)?;
    }
    if request.artifacts.is_empty() {
        return Err(ReleaseEvidenceError::EmptyField("artifacts"));
    }

    let mut names = HashSet::new();
    let mut artifacts = request
        .artifacts
        .iter()
        .map(|path| digest_file(path))
        .collect::<Result<Vec<_>, _>>()?;
    for artifact in &artifacts {
        if !names.insert(artifact.name.clone()) {
            return Err(ReleaseEvidenceError::DuplicateName(artifact.name.clone()));
        }
    }
    artifacts.sort_by(|left, right| left.name.cmp(&right.name));

    let sbom_digest = digest_file(&request.sbom)?;
    if !names.insert(sbom_digest.name.clone()) {
        return Err(ReleaseEvidenceError::DuplicateName(sbom_digest.name));
    }
    let sbom_json = read_json(&request.sbom)?;
    let format = sbom_json
        .get("bomFormat")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let version = sbom_json
        .get("specVersion")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    if format.as_deref() != Some("CycloneDX")
        || version.as_deref() != Some(request.sbom_version.as_str())
    {
        return Err(ReleaseEvidenceError::SbomFormat {
            path: request.sbom.clone(),
            expected: request.sbom_version.clone(),
            format,
            version,
        });
    }
    let sbom = SbomEvidence {
        name: sbom_digest.name,
        sha256: sbom_digest.sha256,
        format: "CycloneDX".into(),
        version: request.sbom_version.clone(),
    };
    Ok(ReleaseManifest {
        schema: RELEASE_EVIDENCE_VERSION,
        source_uri: request.source_uri.clone(),
        source_revision: request.source_revision.clone(),
        builder_id: request.builder_id.clone(),
        artifacts,
        sbom,
        assurance_limitation: request.assurance_limitation.clone().unwrap_or_else(|| {
            "This bundle identifies artifacts, source, SBOM, and a supplied builder ID; it does not by itself establish trusted-builder provenance, signing, or any SLSA Build level.".into()
        }),
    })
}

fn write_evidence(
    manifest: &ReleaseManifest,
    output_directory: &Path,
) -> Result<EvidenceOutputs, ReleaseEvidenceError> {
    let outputs = EvidenceOutputs {
        checksums: output_directory.join("SHA256SUMS"),
        manifest: output_directory.join("opdev-release-manifest.json"),
        provenance: output_directory.join("opdev-provenance.intoto.json"),
    };

    let checksum_lines = manifest
        .artifacts
        .iter()
        .map(|artifact| format!("{}  {}\n", artifact.sha256, artifact.name))
        .chain(std::iter::once(format!(
            "{}  {}\n",
            manifest.sbom.sha256, manifest.sbom.name
        )))
        .collect::<String>();
    write_new(&outputs.checksums, checksum_lines.as_bytes())?;
    let manifest_json = serde_json::to_vec_pretty(&manifest)?;
    write_new_with_newline(&outputs.manifest, &manifest_json)?;

    let subjects = manifest
        .artifacts
        .iter()
        .map(|artifact| ProvenanceSubject {
            name: artifact.name.clone(),
            digest: BTreeMap::from([("sha256", artifact.sha256.clone())]),
        })
        .collect::<Vec<_>>();
    let statement = InTotoStatement {
        statement_type: "https://in-toto.io/Statement/v1",
        subject: &subjects,
        predicate_type: SLSA_PROVENANCE_PREDICATE,
        predicate: ProvenancePredicate {
            build_definition: BuildDefinition {
                build_type: OPDEV_BUILD_TYPE,
                external_parameters: ExternalParameters {
                    source_uri: &manifest.source_uri,
                    source_revision: &manifest.source_revision,
                    sbom: &manifest.sbom,
                },
                internal_parameters: BTreeMap::new(),
                resolved_dependencies: vec![ResourceDescriptor {
                    uri: &manifest.source_uri,
                    digest: BTreeMap::from([("gitCommit", manifest.source_revision.as_str())]),
                }],
            },
            run_details: RunDetails {
                builder: Builder {
                    id: &manifest.builder_id,
                },
                metadata: BTreeMap::new(),
                byproducts: Vec::new(),
            },
        },
    };
    let provenance_json = serde_json::to_vec_pretty(&statement)?;
    write_new_with_newline(&outputs.provenance, &provenance_json)?;
    Ok(outputs)
}

fn validate_non_empty(field: &'static str, value: &str) -> Result<(), ReleaseEvidenceError> {
    if value.trim().is_empty() {
        Err(ReleaseEvidenceError::EmptyField(field))
    } else {
        Ok(())
    }
}

fn portable_name(path: &Path) -> Result<String, ReleaseEvidenceError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ReleaseEvidenceError::MissingName(path.to_path_buf()))
}

fn digest_file(path: &Path) -> Result<FileDigest, ReleaseEvidenceError> {
    let file = fs::File::open(path).map_err(|source| ReleaseEvidenceError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|source| ReleaseEvidenceError::Read {
                path: path.to_path_buf(),
                source,
            })?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(FileDigest {
        name: portable_name(path)?,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

fn read_json(path: &Path) -> Result<serde_json::Value, ReleaseEvidenceError> {
    let bytes = fs::read(path).map_err(|source| ReleaseEvidenceError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| ReleaseEvidenceError::SbomJson {
        path: path.to_path_buf(),
        source,
    })
}

fn write_new_with_newline(path: &Path, bytes: &[u8]) -> Result<(), ReleaseEvidenceError> {
    let mut output = Vec::with_capacity(bytes.len() + 1);
    output.extend_from_slice(bytes);
    output.push(b'\n');
    write_new(path, &output)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), ReleaseEvidenceError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options
        .open(path)
        .map_err(|source| ReleaseEvidenceError::Write {
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(bytes)
        .map_err(|source| ReleaseEvidenceError::Write {
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package_inputs(root: &Path) -> Result<Vec<PackageInput>, Box<dyn std::error::Error>> {
        let plugin = root.join("plugin");
        fs::create_dir_all(plugin.join("hooks"))?;
        fs::write(root.join("opdev"), b"native-binary")?;
        fs::write(plugin.join("README.md"), b"plugin")?;
        fs::write(plugin.join("hooks").join("context.sh"), b"#!/bin/sh\n")?;
        Ok(vec![
            PackageInput {
                source: root.join("opdev"),
                destination: "opdev".into(),
                executable: true,
            },
            PackageInput {
                source: plugin,
                destination: "plugin/opdev".into(),
                executable: false,
            },
        ])
    }

    fn package_bytes(
        inputs: &[PackageInput],
        format: PackageFormat,
        output: &Path,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        package_release(&PackageRequest {
            inputs: inputs.to_vec(),
            format,
            output: output.to_path_buf(),
        })?;
        Ok(fs::read(output)?)
    }

    fn request(root: &Path) -> Result<EvidenceRequest, Box<dyn std::error::Error>> {
        let artifact = root.join("opdev.tar.gz");
        let sbom = root.join("opdev.cdx.json");
        fs::write(&artifact, b"artifact")?;
        fs::write(
            &sbom,
            br#"{"bomFormat":"CycloneDX","specVersion":"1.5","version":1}"#,
        )?;
        Ok(EvidenceRequest {
            artifacts: vec![artifact],
            sbom,
            sbom_version: "1.5".into(),
            source_uri: "https://gitlab.com/example/opdev".into(),
            source_revision: "0123456789abcdef".into(),
            builder_id: "https://gitlab.com/example/opdev/-/runners/1".into(),
            assurance_limitation: None,
            output_directory: root.join("evidence"),
        })
    }

    #[test]
    fn evidence_is_deterministic_and_matches_schemas() -> Result<(), Box<dyn std::error::Error>> {
        let first = tempfile::tempdir()?;
        let second = tempfile::tempdir()?;
        let first_outputs = generate_evidence(&request(first.path())?)?;
        let second_outputs = generate_evidence(&request(second.path())?)?;
        assert_eq!(
            fs::read(&first_outputs.checksums)?,
            fs::read(&second_outputs.checksums)?
        );
        assert_eq!(
            fs::read(&first_outputs.manifest)?,
            fs::read(&second_outputs.manifest)?
        );
        assert_eq!(
            fs::read(&first_outputs.provenance)?,
            fs::read(&second_outputs.provenance)?
        );

        for (path, schema_source) in [
            (
                first_outputs.manifest,
                include_str!("../../../schema/release-manifest.schema.json"),
            ),
            (
                first_outputs.provenance,
                include_str!("../../../schema/provenance.schema.json"),
            ),
        ] {
            let value: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
            let schema: serde_json::Value = serde_json::from_str(schema_source)?;
            let validator = jsonschema::validator_for(&schema)?;
            let errors: Vec<_> = validator.iter_errors(&value).collect();
            assert!(errors.is_empty(), "schema errors: {errors:#?}");
        }
        Ok(())
    }

    #[test]
    fn refuses_wrong_sbom_version_and_existing_outputs() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let first_request = request(root.path())?;
        generate_evidence(&first_request)?;
        assert!(matches!(
            generate_evidence(&first_request),
            Err(ReleaseEvidenceError::Write { .. })
        ));

        let other = tempfile::tempdir()?;
        let mut wrong_version_request = request(other.path())?;
        wrong_version_request.sbom_version = "1.7".into();
        assert!(matches!(
            generate_evidence(&wrong_version_request),
            Err(ReleaseEvidenceError::SbomFormat { .. })
        ));

        let empty_limitation = tempfile::tempdir()?;
        let mut empty_limitation_request = request(empty_limitation.path())?;
        empty_limitation_request.assurance_limitation = Some("  ".into());
        assert!(matches!(
            generate_evidence(&empty_limitation_request),
            Err(ReleaseEvidenceError::EmptyField("assurance_limitation"))
        ));
        Ok(())
    }

    #[test]
    fn release_packages_are_deterministic_and_portable() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let inputs = package_inputs(root.path())?;

        let first_tar = root.path().join("first.tar.gz");
        let second_tar = root.path().join("second.tar.gz");
        assert_eq!(
            package_bytes(&inputs, PackageFormat::TarGz, &first_tar)?,
            package_bytes(&inputs, PackageFormat::TarGz, &second_tar)?
        );
        let decoder = flate2::read::GzDecoder::new(fs::File::open(&first_tar)?);
        let mut archive = tar::Archive::new(decoder);
        let mut tar_entries = Vec::new();
        for entry in archive.entries()? {
            let entry = entry?;
            tar_entries.push((
                entry.path()?.to_string_lossy().into_owned(),
                entry.header().mode()?,
            ));
        }
        assert_eq!(
            tar_entries,
            vec![
                ("opdev".into(), 0o755),
                ("plugin/opdev/README.md".into(), 0o644),
                ("plugin/opdev/hooks/context.sh".into(), 0o755),
            ]
        );

        let first_zip = root.path().join("first.zip");
        let second_zip = root.path().join("second.zip");
        assert_eq!(
            package_bytes(&inputs, PackageFormat::Zip, &first_zip)?,
            package_bytes(&inputs, PackageFormat::Zip, &second_zip)?
        );
        let mut archive = zip::ZipArchive::new(fs::File::open(&first_zip)?)?;
        let mut zip_entries = Vec::new();
        for index in 0..archive.len() {
            let entry = archive.by_index(index)?;
            zip_entries.push((entry.name().to_owned(), entry.unix_mode()));
        }
        assert_eq!(
            zip_entries,
            vec![
                ("opdev".into(), Some(0o100_755)),
                ("plugin/opdev/README.md".into(), Some(0o100_644)),
                ("plugin/opdev/hooks/context.sh".into(), Some(0o100_755)),
            ]
        );
        Ok(())
    }

    #[test]
    fn packaging_refuses_traversal_duplicates_and_overwrite()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("input");
        fs::write(&source, b"input")?;
        let output = root.path().join("output.zip");

        let invalid = PackageRequest {
            inputs: vec![PackageInput {
                source: source.clone(),
                destination: "../escape".into(),
                executable: false,
            }],
            format: PackageFormat::Zip,
            output: output.clone(),
        };
        assert!(matches!(
            package_release(&invalid),
            Err(PackageError::InvalidDestination(_))
        ));

        let duplicate = PackageRequest {
            inputs: vec![
                PackageInput {
                    source: source.clone(),
                    destination: "same".into(),
                    executable: false,
                },
                PackageInput {
                    source: source.clone(),
                    destination: "same".into(),
                    executable: false,
                },
            ],
            format: PackageFormat::Zip,
            output: output.clone(),
        };
        assert!(matches!(
            package_release(&duplicate),
            Err(PackageError::DuplicateDestination(_))
        ));

        let valid = PackageRequest {
            inputs: vec![PackageInput {
                source,
                destination: "input".into(),
                executable: false,
            }],
            format: PackageFormat::Zip,
            output,
        };
        package_release(&valid)?;
        assert!(matches!(
            package_release(&valid),
            Err(PackageError::Write { .. })
        ));
        Ok(())
    }
}
