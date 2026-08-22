# Yanshu Release Supply Chain

Status: current release integrity and provenance contract.

This document defines the v0.11 binary release contract. It does not change the
guest language version, add a capability, publish crates to crates.io, or make
the project production-ready.

## Release identity

The only publishing trigger is a pushed stable tag such as `v0.11.0`. The
release workflow fails closed unless all of the following are true:

- the tag is an annotated Git tag rather than a lightweight ref;
- the tagged commit is contained in `origin/main`;
- the tag is exactly `v` plus `[workspace.package].version`;
- every first-party crate inherits the workspace version, license, edition,
  MSRV, and `publish = false` policy;
- every explicit first-party path dependency pins that same exact version.

Pull requests and manual workflow dispatches run a release rehearsal, but the
`publish` job and its write permissions are unavailable to those events.

## Artifact set

v0.11 builds the `yanshu` CLI and the read-only `yanshu-mcp` server for the two
platforms that are continuously tested by this repository:

| Target | Archive |
| --- | --- |
| `x86_64-unknown-linux-gnu` | `yanshu-vVERSION-x86_64-unknown-linux-gnu.zip` |
| `x86_64-pc-windows-msvc` | `yanshu-vVERSION-x86_64-pc-windows-msvc.zip` |

Each archive contains both executables, README, MIT license, and Apache-2.0
license under one versioned directory. macOS, ARM, installers, dynamic
libraries, and crates.io packages are not implied by this contract.

The final GitHub Release also contains:

- one machine-readable `.build.json` record per target;
- normalized CycloneDX 1.5 SBOMs for the CLI and MCP dependency graphs;
- `yanshu-vVERSION.release.json`, which binds version, source commit, targets,
  artifact sizes, and hashes;
- `SHA256SUMS`, covering every payload and the release manifest.

`SHA256SUMS` proves integrity after a trusted copy has been obtained; it does
not prove who created the file. Authenticity comes from provenance.

## Reproducibility gate

Every platform job builds both executables from two fresh Cargo target
directories and compares each executable independently. Both builds
use `--locked`, an exact Rust release toolchain, disabled incremental builds,
the tag commit timestamp as `SOURCE_DATE_EPOCH`, and a remapped repository path.
reject the release unless both pairs of executable byte streams are exactly
equal. The CLI smoke must return its stable `CLI_USAGE` JSON; the MCP smoke must
complete a real initialize plus `tools/list` exchange and expose exactly the
three read-only source tools. MSVC links with `/Brepro` so the PE timestamp and
CodeView identifier are content-derived instead of wall-clock/random values.
The release profile also fixes codegen units, thin LTO, incremental mode, and
symbol stripping.

Archives use a repository-owned deterministic ZIP writer: entries are sorted,
stored without compression, have fixed timestamps and Unix modes, and contain
only validated relative paths. The SBOM removes its random serial number and
uses the source commit timestamp instead of wall-clock time. Checkout-local
`file:` references are rewritten to source-commit-bound repository identities;
the normalizer rejects any remaining Windows drive or hosted-runner path.

This is a same-source, same-runner double-build proof. GitHub runner images and
system linkers are not hermetic or pinned by image digest, so v0.11 does **not**
claim that an arbitrary independent machine will already reproduce the same
hash. The build record captures the actual Rust and Cargo versions to make such
independent verification possible and to expose drift rather than hide it.

## Keyless provenance

The tag-only publish job has narrowly scoped `contents: write`,
`attestations: write`, and `id-token: write` permissions. It uses GitHub's
OIDC-backed artifact attestation action, pinned to an immutable commit SHA, to
sign provenance for the complete final asset set. No private signing key or
long-lived release credential is stored in the repository.

After downloading a release asset, verify both layers:

```powershell
node scripts/verify-release.mjs <download-directory>
gh attestation verify <download-directory>\yanshu-v0.11.0-x86_64-pc-windows-msvc.zip `
  --repo Yukk1o/yanshu
```

The first command checks the local checksum and manifest closure. The second
checks that GitHub recorded provenance for this repository's workflow. A copied
checksum file without a valid attestation is not trusted release evidence.

## Local rehearsal

Release tooling has dependency-free Node tests:

```powershell
node --test scripts/release.test.mjs
node scripts/release-metadata.mjs
```

Build both native Windows tools twice from clean target directories and create
their shared archive:

```powershell
$commit = git rev-parse HEAD
$epoch = git show -s --format=%ct HEAD
node scripts/build-release.mjs `
  --target x86_64-pc-windows-msvc `
  --out-dir .runtime/release-local `
  --version 0.10.0 `
  --source-commit $commit `
  --source-date-epoch $epoch `
  --allow-dirty
```

`--allow-dirty` marks the build record as non-publishable; the assembler rejects
it. Omit that flag after committing to reproduce a publishable tree. Generated
rehearsal files stay under ignored `.runtime/`. A real release is
created only after the normal release gates pass, the workspace version is
reviewed, the commit is merged to `main`, and an annotated matching tag is
pushed. The workflow deliberately fails if a GitHub Release already exists;
published tags and assets are never silently overwritten.

## Trust boundary and remaining work

The release workflow packages the trusted Rust CLI and read-only MCP host; it
never runs an Agent Backend, provider request, guest service, production
capability, or version promotion. Pull-request code cannot reach the tag-only
write-token job.
Checkout credentials are not persisted, every Action is pinned to a full
commit SHA, and cargo-cyclonedx is installed at an exact locked version.

Remaining limitations include independently operated rebuilders, hermetic
runner images, macOS/ARM artifacts, hardware-backed maintainer tag signatures,
release-environment approval rules, and long-term transparency mirroring.
