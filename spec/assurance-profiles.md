# Assurance profiles

OpDev assurance profiles are exact-version compatibility documents. They map
project evidence to an internal baseline, an external framework, or an evidence
format. They never make a conformance claim merely because a project selects a
profile.

The built-in profiles are:

| Profile | Interpretation |
| --- | --- |
| `opdev-core@1` | Normative OpDev and MinimumCD rule catalog. |
| `nist-ssdf-derived@1.1` | Informative mapping to final NIST SP 800-218 SSDF 1.1. |
| `slsa-build-provenance@1.2` | SLSA 1.2 provenance evidence mapping, without a Build level claim. |
| `cyclonedx-sbom@1.5` | CycloneDX 1.5 SBOM evidence supported by the pinned Rust generator. |
| `openssf-osps-baseline-derived@2026.02.19` | Incomplete informative mapping to that exact OSPS Baseline release. |

`opdev profiles` lists the profiles bundled into the current binary. A project
contract must select an exact name and version. `latest` aliases are not
accepted. Unsupported versions produce a project-contract error so an upgrade
cannot silently change assurance semantics.

Mappings use `full`, `partial`, and `gap` coverage labels. These labels describe
the relationship between rules and framework text, not the evaluated state of a
particular project. Even `full` coverage requires evidence-backed rule results.
Passing mapped rules does not establish organization-wide governance, assessor
approval, trusted-builder properties, or third-party certification.

External standards advance independently of OpDev. A new standards version is
introduced as a new profile document and reviewed like a schema migration. Old
profiles remain stable for reproducibility until a future compatibility policy
explicitly removes them.
