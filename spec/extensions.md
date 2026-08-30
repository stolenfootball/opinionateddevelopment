# OpDev extension protocol 1.0

OpDev v1 supports project-owned command checks as an additive extension mechanism. Extensions are declared in `.opdev/project.yaml`, reference a canonical command by key, and run only at their declared lifecycle stage. The CLI launches each argument vector directly without a shell, applies the declared timeout, contains the subprocess tree, and bounds captured output.

An extension receives one JSON request on standard input conforming to `schema/extension-request.schema.json`. A successful extension process exits zero and writes exactly one JSON response conforming to `schema/extension-response.schema.json` on standard output. Diagnostic logging belongs on standard error. A nonzero exit, timeout, incompatible protocol version, malformed response, or empty summary produces `error`; it never becomes a product `failed` result or an implicit pass.

The response uses the same exhaustive outcomes as core rules. Only `passed` and demonstrably justified `not_applicable` satisfy a blocking extension. Non-blocking extensions remain visible but do not affect a gate.

Extensions cannot target, replace, suppress, reinterpret, or change the applicability of core rule IDs. Their project-local IDs occupy a separate result collection. Core gate aggregation runs independently before additive blocking checks are included. This structural separation enforces `OPDEV-EXT-001` without requiring every project to learn a general policy language.

Versioned declarative rule packs may be added in a later protocol revision. A sandboxed WASM verifier remains a possible future option for portable checks that cannot be expressed declaratively. Native dynamic libraries and unrestricted policy languages are intentionally outside the core extension boundary.
