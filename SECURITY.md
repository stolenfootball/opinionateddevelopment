# Security policy

## Reporting a vulnerability

Do not open a public issue for a suspected vulnerability. Use GitLab's private
vulnerability reporting flow for this project. Include the affected version,
reproduction details, impact, and any suggested mitigation. Avoid including
credentials, personal data, or unrelated secrets.

Maintainers should acknowledge a complete report within five business days,
keep the reporter informed while impact and remediation are assessed, and
coordinate disclosure after a fix or mitigation is available. These are
response targets, not a guarantee that every issue can be resolved within a
fixed period.

## Supported versions

Until OpDev reaches its first stable release, only the latest published release
and the current default branch receive security fixes. Release notes will state
when this policy changes.

## Security boundaries

The CLI treats initialized project content as untrusted:

- discovery does not execute repository-controlled commands;
- configured checks use exact argument vectors without a shell;
- checks have time and output bounds and terminate their process group;
- remote audits are read-only and limited to first-class provider hosts;
- extensions cannot replace or weaken core MinimumCD results; and
- release evidence does not claim signing or trusted-builder provenance.

Running an initialized project's canonical commands still executes code chosen
by that project. Review `.opdev/project.yaml` before running checks from an
untrusted repository.
