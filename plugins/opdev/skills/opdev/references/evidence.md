# Reviewable evidence

Use `.opdev/evidence.yaml` only when a core rule accepts evidence and the CLI
cannot infer the fact safely. It is not a waiver or override mechanism.

For change evidence:

1. Stage every material file except that the ledger may remain unstaged.
2. Run `opdev evidence fingerprint`.
3. Add assertions under a change entry with that fingerprint and a concrete work authority.
4. Cite inspectable facts from code, tests, specifications, CI, review, or the work system.
5. Stage the ledger and run the applicable check.

The fingerprint excludes only the ledger. Unstaged tracked content and other
untracked files cause fingerprinting to fail. Any later material repository
change invalidates the assertions automatically.

Project assertions are for durable capabilities or policies, not current-change
acceptance. Use them sparingly. An agent's statement that its own output is
correct is never sufficient evidence. Evidence cannot override a concrete
failure, error, or migration requirement.
