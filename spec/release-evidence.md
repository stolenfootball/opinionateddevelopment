# Release evidence

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

The release sequence is:

```text
build immutable artifact
  -> generate matching CycloneDX SBOM
  -> run opdev release evidence
  -> publish artifact + SBOM + checksums + manifest + provenance together
  -> verify digests after download
```

Generation alone does not pass `OPDEV-SUPPLY-002`. The delivery gate also needs
evidence that the bundle was published with the artifact and that the chosen
distribution and authenticity controls match the project's risk model.
