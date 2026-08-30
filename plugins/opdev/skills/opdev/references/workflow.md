# OpDev lifecycle

## Specify

Identify the problem, intended outcome, affected consumers, scope, exclusions, acceptance conditions, evidence, authorities, dependencies, and material risks. Put active status in the declared work system.

## Design

Scale design to novelty, reversibility, blast radius, data and security consequences, and operational risk. Durable architecture or contract decisions record the generalized problem, alternatives, rationale, evidence, canonical authority update, and reversal trigger.

## Implement and integrate

Use one trunk. Branches originate from trunk, remain short-lived, integrate at least daily during active work, and are removed after merge. Stop feature work while required trunk CI is red; diagnosis and restoration take priority.

Run relevant local and pre-merge suites. Preserve supported delivered behavior, or record and test an intentional migration. Agent-authored work meets exactly the same standards as human-authored work.

## Package and deliver

CI is the exclusive supported delivery path. The pipeline gives a definitive verdict, builds a deployable artifact once, identifies it immutably, and promotes the same bytes. Qualify in an environment representative of material destination risks. Version and test behavioral configuration; inject environment-specific values without rebuilding.

Use one consumer-facing delivery path and an automated, tested recovery strategy appropriate to the software: rollback, previous-artifact redeploy, disablement, restoration, safe roll-forward, or a focused forward fix when reversal is unsafe.

## Operate and learn

For operated software, collect user-centered health and diagnostic evidence after delivery. Reconcile defects, incidents, decisions, tests, and the project contract. Evaluate intended effectiveness separately from deterministic correctness.

## Gates

The four aggregates are development, integration, delivery, and compliance. A gate is blocked when any applicable required rule or blocking project check is not `passed` or justified `not_applicable`. A report may be useful even while blocked; never summarize it as successful.
