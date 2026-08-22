# Yanshu Release Supply Chain

Status: current release integrity and provenance contract.

This document defines the current binary and editor release contract. v0.11
established the CLI/MCP evidence chain; v0.12 extends the same closed manifest
to the LSP and platform VSIX. It does not change the guest language version,
add a capability, publish crates to crates.io or the VS Code Marketplace, or
make the project production-ready.

## Release identity

The only publishing trigger is a pushed stable tag such as `v0.12.0`. The
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

The workflow builds the `yanshu` CLI, read-only `yanshu-mcp` server, and
`yanshu-lsp` for the two platforms that are continuously tested by this
repository. It also packages one VSIX carrying that target's LSP:

| Target | Archive | VSIX |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | `yanshu-vVERSION-x86_64-unknown-linux-gnu.zip` | `yanshu-vscode-VERSION-linux-x64.vsix` |
| `x86_64-pc-windows-msvc` | `yanshu-vVERSION-x86_64-pc-windows-msvc.zip` | `yanshu-vscode-VERSION-win32-x64.vsix` |

Each archive contains all three executables, README, MIT license, and
Apache-2.0 license under one versioned directory. Each VSIX contains the
bundled extension client, language contributions, notices, and exactly one
matching LSP binary. macOS, ARM, installers, dynamic libraries, Marketplace
publication, and crates.io packages are not implied by this contract.

The final GitHub Release also contains:

- one machine-readable `.build.json` record per target;
- normalized CycloneDX 1.5 SBOMs for the CLI, MCP, and LSP Rust dependency
  graphs, plus the extension's locked production npm graph;
- `yanshu-vVERSION.release.json`, which binds version, source commit, targets,
  artifact sizes, and hashes;
- `SHA256SUMS`, covering every payload and the release manifest.

`SHA256SUMS` proves integrity after a trusted copy has been obtained; it does
not prove who created the file. Authenticity comes from provenance.

## Reproducibility gate

Every platform job builds all three executables from two fresh Cargo target
directories and compares each executable independently. Both builds
use `--locked`, an exact Rust release toolchain, disabled incremental builds,
the tag commit timestamp as `SOURCE_DATE_EPOCH`, and a remapped repository path.
The job rejects the release unless all three pairs of executable byte streams
are exactly equal. The CLI smoke must return its stable `CLI_USAGE` JSON; the
MCP smoke must complete a real initialize plus `tools/list` exchange and expose
exactly the three read-only source tools; the LSP smoke must complete framed
`initialize`, `shutdown`, and `exit`, negotiate UTF-16, and report the read-only
review contract. MSVC links with `/Brepro` so the PE timestamp and CodeView
identifier are content-derived instead of wall-clock/random values. The
release profile also fixes codegen units, thin LTO, incremental mode, and
symbol stripping.

The extension uses an exact locked `@vscode/vsce` version and Node 22. The job
packages the VSIX twice, once with each independently built LSP, under the same
`SOURCE_DATE_EPOCH`, then compares both VSIX byte streams. Its build record
binds the VSIX digest to the bundled LSP digest and platform. A package that is
merely buildable in editor CI is not a release asset until this gate and the
final manifest closure both pass.

Archives use a repository-owned deterministic ZIP writer: entries are sorted,
stored without compression, have fixed timestamps and Unix modes, and contain
only validated relative paths. Both Rust and npm SBOM normalizers remove random
serial numbers and use the source commit timestamp instead of wall-clock time.
Checkout-local `file:` references are rewritten to source-commit-bound
repository identities; the normalizers reject any remaining Windows drive or
hosted-runner path. The npm root is also marked as bundling the LSP, so the
extension dependency graph cannot be mistaken for the complete executable
closure.

This is a same-source, same-runner double-build proof. GitHub runner images and
system linkers are not hermetic or pinned by image digest, so this contract does **not**
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

Install the exact extension graph first, then build all three native Windows
tools and the platform VSIX twice from clean target directories and create the
shared archive:

```powershell
Push-Location editors\vscode
npm ci --registry=https://registry.npmjs.org
npm audit --registry=https://registry.npmjs.org --audit-level=high
npm run check
Pop-Location

$commit = git rev-parse HEAD
$epoch = git show -s --format=%ct HEAD
node scripts/build-release.mjs `
  --target x86_64-pc-windows-msvc `
  --out-dir .runtime/release-local `
  --version 0.12.0 `
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

The release workflow packages the trusted Rust CLI, read-only MCP host, bounded
LSP, and extension client; it never opens a workspace document or runs an Agent
Backend, provider request, guest service, production capability, or version
promotion. Pull-request code cannot reach the tag-only write-token job.
Checkout credentials are not persisted, every Action is pinned to a full
commit SHA, and cargo-cyclonedx is installed at an exact locked version.

Remaining limitations include independently operated rebuilders, hermetic
runner images, macOS/ARM artifacts, hardware-backed maintainer tag signatures,
release-environment approval rules, and long-term transparency mirroring.
