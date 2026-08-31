# Release operations

The protected GitLab tag pipeline is the only supported way to publish OpDev.
It mirrors the exact tag revision to GitHub, dispatches ephemeral native GitHub
builders, retrieves their immutable archives, and then generates evidence,
signs, and publishes from GitLab. GitHub Actions artifacts are an internal build
handoff, not a supported distribution path. Do not upload, replace, or
reconstruct release assets manually.

GitLab is the source and release authority. The protected tag pipeline
fast-forwards the public GitHub build mirror's `main` branch to the exact
qualified revision before dispatch and deletes the ephemeral
`gitlab-release/<tag>` source branch after the artifact handoff. GitHub releases
and GitHub Actions artifacts are not consumer distribution channels.

## Candidate and final release

1. Merge a green change pipeline to `main` and confirm the resulting trunk
   pipeline is green.
2. Create an annotated candidate tag such as `v0.1.1-rc.1` on that exact trunk
   revision and push it to GitLab.
3. Confirm all six native archives and the plugin archive were smoke-tested,
   reproduced byte-for-byte, included in `SHA256SUMS`, signed, and published
   with the SBOM, manifest, and provenance.
4. Install the candidate archives on representative consumer systems and run
   `opdev version`, `opdev init --dry-run`, and a fixture `opdev check`.
5. If the candidate is accepted, create the final `v0.1.1` tag on the same
   qualified revision. The final tag runs the complete pipeline again; it does
   not promote candidate bytes under a new identity.

The separate final build is intentional because the versioned asset names,
source revision, and tag-bound signing identity differ. Within each tag
pipeline, every published archive is built once on its target runner and
promoted unchanged through the GitHub artifact handoff, GitLab evidence,
signing, and publication.

## Consumer verification

Download the selected archive, its `.sigstore.json` bundle, and `SHA256SUMS`
from the same GitLab release. Verify the digest and then the GitLab signing
identity:

```sh
sha256sum -c SHA256SUMS --ignore-missing
cosign verify-blob opdev-0.1.1-x86_64-unknown-linux-gnu.tar.gz \
  --bundle opdev-0.1.1-x86_64-unknown-linux-gnu.tar.gz.sigstore.json \
  --certificate-identity "https://gitlab.com/stolenfootball-tools/opinionateddevelopment//.gitlab-ci.yml@refs/tags/v0.1.1" \
  --certificate-oidc-issuer "https://gitlab.com"
```

Use the matching file names on Windows or macOS. `opdev-release-manifest.json`
records the exact source revision, builder pipeline, artifact digests, SBOM
digest, and the scope limitation of the all-target dependency inventory.

## Recovery

OpDev is a stateless CLI and agent plugin. Consumers select an exact version,
and published releases are immutable, so an unsafe release does not require a
data rollback or mutation of existing assets.

Recovery is an on-demand safe roll-forward:

1. Mark the affected release and its known impact in GitLab without deleting
   its evidence.
2. Restore `main` first if its required pipeline is red.
3. Implement the smallest compatible fix with a regression test.
4. Run the normal merge-request and trunk gates.
5. Publish a new candidate and then a new SemVer patch through the tag pipeline.
6. Repeat checksum, signature, installation, and fixture checks before advising
   consumers to upgrade.

Until the forward fix qualifies, consumers can pin or reinstall the last known
good exact version. This procedure must be exercised during the initial release
candidate and whenever the delivery path materially changes.
