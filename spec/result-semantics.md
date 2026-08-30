# Result and aggregation semantics

Every OpDev rule evaluation produces exactly one result.

## Results

### `passed`

The rule applied, its verifier completed successfully, and all evidence required
by that verifier is present and valid for the evaluated subject.

### `failed`

The rule applied and evidence demonstrates that its requirement was not met.
A failure cannot be overridden by an extension or converted to a warning.

### `unverified`

The rule appears applicable, but OpDev lacks sufficient evidence or permission
to decide whether it passed. Missing remote permissions, missing live evidence,
and stale qualification evidence are unverified rather than passed.

### `not_applicable`

The rule's declared applicability condition is demonstrably false for the
evaluated project or change. A reason and the evidence used to decide
applicability are required.

### `error`

OpDev could not complete the evaluation because the verifier, project command,
provider, or evidence parser failed. Errors are not project failures, but they
block any verdict that depends on the affected rule.

### `migration_required`

A brownfield project does not yet implement the rule and has recorded the gap as
migration work. This permits incremental development but is not compliance and
cannot qualify a delivery governed by that rule.

## Required result record

Every result MUST identify:

- Rule ID and rule-catalog version.
- Result.
- Subject, such as project, revision, artifact digest, environment, or change.
- Verifier and verifier version.
- Evaluation time.
- Evidence references or a reason no evidence was available.
- A concise diagnostic.

Results that execute a command also record the argument vector, exit
classification, duration, and redacted output reference. Secrets and sensitive
values MUST NOT be embedded in result records.

## Aggregation

A required rule contributes to an aggregate verdict as follows:

| Rule result | Aggregate effect |
| --- | --- |
| `passed` | Satisfies the rule. |
| `not_applicable` | Satisfies aggregation only when applicability evidence is valid. |
| `failed` | Blocks the verdict. |
| `unverified` | Blocks the verdict. |
| `error` | Blocks the verdict. |
| `migration_required` | Blocks compliance and qualified delivery, but may permit local development. |

An aggregate verdict passes only when every applicable required rule is either
`passed` or validly `not_applicable`.

## Gates

Rules may participate in one or more gates:

- `development`: whether ordinary local implementation may proceed.
- `integration`: whether a change may enter trunk.
- `delivery`: whether an artifact may be delivered through OpDev.
- `compliance`: whether the project may claim the selected OpDev or external
  profile.

A broken required trunk pipeline blocks new feature development even when local
development checks would otherwise pass. Repair, diagnosis, rollback, and work
needed to restore the pipeline remain permitted.

## Freshness and identity

Evidence is valid only for the subject it identifies. Artifact qualification
MUST identify the artifact digest. Revision-only evidence cannot be silently
reused for a different artifact. Time-sensitive or external evidence MUST state
its freshness policy.

## Extensions

Extensions may produce additional results and may strengthen a gate. They MUST
NOT:

- Replace a core result.
- Change a core rule's applicability.
- convert `failed`, `unverified`, `error`, or `migration_required` to `passed`.
- Suppress required evidence.

