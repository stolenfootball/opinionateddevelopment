# Contributing to OpDev

OpDev accepts focused changes that preserve its evidence semantics and remain
applicable to software projects beyond the repository used to motivate them.

Before editing, read `.opdev/project.yaml`, the authorities selected by its
context routes, and the tracked work item. The work item should define the
problem, intended outcome, scope and exclusions, acceptance conditions, evidence,
and material risks. Durable architecture or contract changes belong in `spec/`
and should describe alternatives, rationale, and a reversal trigger.

Use Rust 1.97.0 and run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Behavioral changes require automated tests. Escaped defects require regression
coverage unless a specific, reviewable limitation explains why automation cannot
prevent recurrence. Do not hide flakes with retries; a quarantine needs an owner,
tracked remediation, and an expiry.

Before requesting integration, stage all material files and use
`opdev evidence fingerprint` to bind any necessary change assertions in
`.opdev/evidence.yaml`. Evidence must point to concrete repository, CI, work, or
review facts. An agent's claim that its own output is correct is not evidence.

Use conventional commit messages. Keep branches short-lived and integrate only
through CI. Release tags are `vMAJOR.MINOR.PATCH`; their pipeline builds the
artifact once and publishes its SBOM, checksums, release manifest, and provenance
with the same immutable archive.
