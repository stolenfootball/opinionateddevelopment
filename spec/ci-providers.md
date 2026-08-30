# CI provider boundary

GitHub Actions and GitLab CI are first-class providers in OpDev 0.1. Each adapter owns four responsibilities: the canonical configuration path, baseline rendering, read-only local inspection, and evidence describing change and trunk qualification. Provider-specific syntax remains behind the `CiAdapter` boundary; the rule engine consumes provider-neutral outcomes.

`opdev ci generate` prints a configuration by default and writes only with `--write`. A write uses create-new semantics and refuses to replace an existing CI file. This makes adoption reviewable in brownfield projects. `opdev ci inspect` parses configuration as YAML without executing it. `opdev check --ci` folds those findings into the relevant MinimumCD rule results before recomputing the integration gate.

Generated configurations run for proposed changes and the declared trunk, preserve the JSON report even when a gate blocks, use read-only repository permissions, install an exact OpDev release, and verify the selected native archive against the release checksum manifest. GitHub third-party actions are pinned to full commit identifiers. Provider configuration and immutable release publication remain separate authorities.

Additional providers implement the same Rust trait or, in a future external rule-pack protocol, contribute equivalent evidence without altering core rule IDs. Unknown providers remain `unverified`; they never inherit a pass from GitHub or GitLab assumptions.
