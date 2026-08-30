# OpDev normative specification

This directory defines how OpDev interprets and evaluates the machine-readable
requirements in [`rules/core.yaml`](../rules/core.yaml). The rule catalog is the
authoritative home for individual requirements. This specification is the
authoritative home for cross-rule semantics.

OpDev is a development operating system for general software projects. It
combines an evidence-driven development workflow with the delivery constraints
published by [MinimumCD](https://minimumcd.org/).

## Normative language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHOULD**, **SHOULD NOT**,
and **MAY** are interpreted as described by RFC 2119 and RFC 8174 when, and only
when, they appear in capitals.

## Authority order

When two OpDev sources appear to conflict, use this order:

1. The versioned rule statement in `rules/core.yaml`.
2. Result and aggregation semantics in `spec/result-semantics.md`.
3. Compatibility rules in `spec/compatibility.md`.
4. A project's valid `.opdev/project.yaml` contract.
5. Platform adapters, templates, generated documentation, and agent guidance.

Project contracts select applicable profiles and implementations. They cannot
weaken a core rule or redefine a core failure as passing.

Schema-validated project evidence follows [`evidence-ledger.md`](evidence-ledger.md).
It may satisfy an otherwise unverified rule only when the catalog explicitly
allows evidence verification; it cannot override a concrete failure, error, or
migration requirement.

## Lifecycle

OpDev uses this general lifecycle:

```text
Understand -> Specify -> Design -> Implement -> Verify
           -> Integrate -> Package -> Deliver -> Observe -> Learn
```

The lifecycle describes outcomes rather than prescribing a framework, hosting
platform, programming language, repository topology, or work tracker.

## Three independent verdicts

OpDev keeps these judgments separate:

- **Correctness**: the software does what its contracts and acceptance criteria
  say under the tested conditions.
- **Deployability**: the exact artifact can be delivered and recovered through
  the declared automated path.
- **Effectiveness**: measured use or evaluation shows that the software solves
  the intended problem.

A project can be correct and deployable without yet having effectiveness
evidence. It cannot be called deployable when a required delivery check is
missing, stale, or unverified.

## Proportional evidence

OpDev scales evidence to the risk and durability of a change:

- Routine changes use an executable work item, automated tests, and merge
  evidence.
- Contract changes also update the contract's canonical authority.
- Durable architecture changes also record alternatives, rationale, evidence,
  and a reversal trigger.
- Specialized security, accessibility, performance, safety, or effectiveness
  evidence is required when the corresponding risk or profile applies.

Proportionality can reduce ceremony. It cannot remove applicable MinimumCD
requirements or turn unknown evidence into a passing result.

## External standards

OpDev uses public guidance from MinimumCD, NIST SSDF, SLSA, W3C WCAG, OWASP,
OpenSSF, and the public descriptions of ISO testing and quality models. OpDev
only claims conformance to an external standard when a versioned profile
contains every applicable requirement and the project supplies complete
evidence. Referencing or deriving a practice is not a conformance claim.
