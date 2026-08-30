# Release evidence

`opdev release package` creates deterministic `.tar.gz` and `.zip` archives
from explicit source-to-destination mappings. Inputs are sorted, archive
timestamps and owner identities are normalized, permissions are fixed, and
existing outputs are never replaced. Symlinks, path traversal, duplicate
destinations, and unsupported input types are rejected. Release CI builds each
binary once and uses this command to package those exact bytes; a second package
operation must produce an identical archive before qualification continues.

OpDev release evidence binds already-built artifacts to four facts:

1. each artifact's SHA-256 digest;
2. the SHA-256 digest and exact version of a CycloneDX JSON SBOM;
3. an exact source URI and revision; and
4. a builder identifier supplied by the build environment.

`opdev release evidence` creates three files without replacing existing output:

- `SHA256SUMS` for artifacts and the SBOM;
- `opdev-release-manifest.json` for the complete association and its limitation;
- `opdev-provenance.intoto.json`, an in-toto Statement v1 using the SLSA
  provenance v1 predicate and the OpDev release build type.

The output is deterministic for the same byte inputs and arguments. It omits
timestamps and invocation identifiers because the command runs after the build
and cannot honestly attest when or how the build began. The release platform
may add stronger, contemporaneous provenance separately.

The evidence is not signed by the CLI. A builder ID passed on the command line
is a claim from the caller, not proof of builder identity. The files therefore
do not establish a SLSA Build level, trusted-builder isolation, provenance
authenticity, secure distribution, or successful consumer verification.

Canonical GitLab tag pipelines add a separate keyless Sigstore bundle for each
native and plugin archive. The signing job receives a short-lived GitLab OIDC token with
the `sigstore` audience, records the signing identity through Fulcio/Rekor, and
verifies the bundle against the exact GitLab project, `.gitlab-ci.yml`, and tag
identity before publication. This authenticates the archive at release time;
it does not retroactively turn the CLI-generated provenance into trusted-builder
provenance or establish a SLSA Build level.

## Rust release SBOM

OpDev pins `cargo-cyclonedx` 0.5.9 with `--locked` and emits CycloneDX JSON 1.5,
the newest specification version that generator supports. The broader
CycloneDX standard is newer; OpDev does not label 1.5 output as 1.7. The release
configuration is machine-readable in `release/supply-chain.toml`.

An SBOM should be generated with the same feature and target configuration as
the artifact it describes. If a single all-target SBOM is associated with
multiple platform artifacts, the release evidence must retain the limitation
that it is a superset inventory rather than an exact per-artifact dependency
graph.

The canonical release matrix builds and smoke-tests six targets in native or
production-like runner environments:

- Windows MSVC on x86-64, with ARM64 cross-built using Microsoft's ARM64 tools
  and its PE machine type verified;
- Linux GNU on native x86-64 and ARM64 runners; and
- macOS on Apple Silicon, with both ARM64 and x86-64 binaries exercised (the
  latter under Rosetta).

The plugin distribution is packaged independently of the native CLI archives
and validated for both Codex and Claude Code. A target is supported only when
its archive, checksum, signature bundle, SBOM association, manifest, and
provenance are published together by the canonical tag pipeline.

The release sequence is:

```text
build immutable artifact
  -> generate matching CycloneDX SBOM
  -> run opdev release evidence
  -> keyless-sign and verify the archive in canonical release CI
  -> publish artifact + signature + SBOM + checksums + manifest + provenance together
  -> verify digests after download
```

Release-candidate and final tags use the same immutable pipeline. Recovery for
the stateless CLI and plugin is a safe roll-forward: repair the source on trunk,
repeat the complete qualification matrix, and publish a new SemVer tag. Existing
release assets are never replaced. Consumers can continue using a prior exact
version until the forward fix qualifies. The operational procedure and
verification commands are defined in `release/README.md`.

Generation alone does not pass `OPDEV-SUPPLY-002`. The delivery gate also needs
evidence that the bundle was published with the artifact and that the chosen
distribution and authenticity controls match the project's risk model.
