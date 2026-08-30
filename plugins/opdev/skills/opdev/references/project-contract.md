# Project contract interpretation

`.opdev/project.yaml` is a versioned, strict schema. Unknown fields and unsafe paths fail validation rather than being ignored.

- `project` declares a general software kind, one integration trunk, the CI provider, and the read-only remote.
- `authorities` maps important fact categories to one repository path, URL, or tracker. These are locations, not mandatory folder names.
- `commands` contains shell-free argument vectors, optional project-relative working directories, and timeouts.
- `quality.risks` selects the characteristics that drive acceptance and testing depth.
- `testing` declares change and regression policy, flake visibility, coverage strategy, and suites by lifecycle stage.
- `delivery` describes the consumer-facing action, immutable artifact identity, representative environments, and recovery strategy. `migration_required` is a tracked gap, never compliance. `configured` is a reviewed assertion that the declared CI provider is the single supported delivery path; it requires a recovery strategy and at least one production-like qualification environment, while concrete pipeline and artifact claims still need independent evidence.
- `operations` routes health and observability evidence for operated software.
- `assurance.profiles` pins optional derived guidance by name and version.
- `extensions.checks` adds project-owned protocol commands. Blocking extensions affect their declared gate but remain separate from core rule results.
- `context` routes each task to only the authorities it needs. `always` is the small baseline.

When project facts change, update their declared authority and then reconcile pointers in the contract. Do not create a second authoritative copy for agent convenience.
