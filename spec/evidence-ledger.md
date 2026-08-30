# Project evidence ledger

`.opdev/evidence.yaml` is an optional, schema-validated ingress for facts the
CLI cannot infer safely. It is not a waiver file. It can assert only `passed` or
`not_applicable`, must include concrete evidence, and can address only rules
whose catalog verification methods permit `evidence` or reviewed `agent` facts.

Project assertions describe durable policy or capability and should be used
sparingly. Change assertions are bound to an exact SHA-256 fingerprint of the
staged Git index. The fingerprint includes each tracked path, mode, stage, and
Git blob identity while excluding only `.opdev/evidence.yaml`, allowing the
ledger to be written after the fingerprint is calculated.

The safe sequence is:

1. stage every material file for the change;
2. run `opdev evidence fingerprint`;
3. add or replace the matching change entry in `.opdev/evidence.yaml`;
4. stage the ledger and run `opdev check` or `opdev check --ci`; and
5. review and commit the evidence with the files it describes.

Fingerprinting fails when tracked changes are unstaged or material untracked
files exist. CI checks out the committed index and recomputes the same value.
Any future path, content, or executable-bit change produces a different
fingerprint, so stale change assertions are ignored and required rules return
to `unverified`.

Evidence is applied only when the normal evaluator returned `unverified`.
It cannot override an explicit failure, error, migration requirement, manifest
contradiction, CI result, or remote-provider result. Project extensions remain
structurally separate and cannot write core rule results.

Example:

```yaml
schema: 1
project:
  - rule_id: OPDEV-SEC-001
    outcome: passed
    summary: The reviewed security policy defines the project trust boundaries.
    evidence:
      - kind: policy
        summary: Reporting, command execution, and remote-audit boundaries are documented.
        location: SECURITY.md
changes:
  - fingerprint: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
    work: https://example.test/project/issues/42
    assertions:
      - rule_id: OPDEV-WORK-001
        outcome: passed
        summary: The work item defines scope, acceptance conditions, evidence, and risks.
        evidence:
          - kind: work
            summary: Issue 42 is the accepted executable work authority.
            location: https://example.test/project/issues/42
```

An assertion is still a reviewed project claim. Fingerprinting prevents reuse
against different bytes; it does not prove that a cited review was competent or
that an external URL is authentic. Higher-assurance projects can add stricter
extension checks or signed attestations without weakening this core behavior.

## Design decision

The generalized problem is that strict gates need human or project evidence,
but a permanent unbound assertion can silently qualify later changes. Weakening
gate aggregation would violate OpDev semantics, while requiring an external
attestation service would make initialization provider-specific and burdensome.
Binding assertions directly to the final Git commit is circular because the
ledger itself changes that commit.

Version 1 therefore fingerprints the staged Git index while excluding only the
ledger. This preserves strict aggregation, works in every Git-hosted software
project, is reviewable in the same change, and invalidates itself when material
bytes or modes change. The tradeoff is that assertions remain project claims
rather than cryptographically authenticated third-party attestations.

Revisit this decision when common CI providers can supply portable, signed,
change-level review attestations with equivalent local developer ergonomics. A
future protocol may accept those attestations alongside the ledger, but it must
not reinterpret existing version 1 fingerprints or silently trust unbound data.
