# Testing requirements

Derive tests from behavior, acceptance conditions, and declared quality risks rather than from language-specific quotas.

- Every behavioral change needs automated verification unless existing coverage is demonstrated or a specific limitation is recorded.
- Every escaped defect needs regression protection unless a specific justification explains why it is impractical or harmful.
- Required suites run before integration and again on integrated trunk. Delivery, package, recovery, scheduled, and evaluation suites run at their declared stages.
- A retry remains visible. Quarantine requires an owner, tracked remediation, and an expiry. A flaky or unavailable qualification test cannot silently qualify delivery.
- Coverage identifies untested risk. Use the project-selected mode—reporting, non-regression, changed-code threshold, or critical-module thresholds—without treating percentage alone as test quality.
- Tests that depend on live services, devices, stores, fleets, models, or other external systems must state dependencies, environment, variability, freshness, and whether their result affects deployability or effectiveness.
- Keep deterministic correctness and deployability separate from effectiveness evaluation.

Run only canonical commands relevant to the current stage. Preserve their bounded output as evidence, and distinguish a product failure (`failed`) from a verifier failure (`error`). Keep commands as exact argument vectors. On Windows, OpDev may resolve the allowlisted Node package-manager names through `PATH` and executable `PATHEXT` shims; do not replace that constrained behavior with a hand-built shell command.
