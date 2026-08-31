# Reviewable evidence

Use `.opdev/evidence.yaml` only when a core rule accepts evidence and the CLI
cannot infer the fact safely. It is not a waiver or override mechanism.

For change evidence:

1. Stage every material file except that the ledger may remain unstaged.
2. When no ledger exists, run `opdev evidence bootstrap` and save its output
   outside the Git working tree. Every decision starts as `review_required`.
3. Review the generated project and change candidates. Add concrete shared
   evidence to each used scope, provide a change work authority, and replace a
   decision only with a justified `passed` or `not_applicable`.
4. Preview the expanded ledger with `opdev evidence bootstrap --answers PATH`.
   Create it only after review with the same command plus `--write`.
5. Stage the ledger and run the applicable check.

Bootstrap is create-new only. It validates the questionnaire schema, exact
candidate set, staged fingerprint, evidence, and rule support. It never chooses
a satisfying result and cannot update an existing ledger. For direct ledger
maintenance, use `opdev evidence fingerprint`, then add assertions under the
matching change entry with a concrete work authority.

The fingerprint excludes only the ledger. Unstaged tracked content and other
untracked files cause fingerprinting to fail. Any later material repository
change invalidates the assertions automatically.

Project assertions are for durable capabilities or policies, not current-change
acceptance. Use them sparingly. An agent's statement that its own output is
correct is never sufficient evidence. Evidence cannot override a concrete
failure, error, or migration requirement.
