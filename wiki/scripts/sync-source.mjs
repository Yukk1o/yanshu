import { copyFile, mkdir, rm } from 'node:fs/promises'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const wikiRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repositoryRoot = resolve(wikiRoot, '..')
const outputRoot = join(wikiRoot, 'public', 'source')

// These are snapshots of the real project files, not hand-maintained excerpts.
// Keeping the list explicit prevents the documentation build from publishing
// credentials, runtime data, or unrelated workspace files.
const publishedFiles = [
  'Cargo.lock',
  'Cargo.toml',
  'deny.toml',
  'README.md',
  'TASKS.md',
  'conformance/v1/invalid/multiple-forms.ail',
  'conformance/v1/invalid/unknown-library.ail',
  'conformance/v1/manifest.json',
  'conformance/v1/programs/core.ail',
  'conformance/v1/programs/library.ail',
  'conformance/v1/programs/schema.ail',
  'docs/business-backend-v0.3.md',
  'docs/conformance-v1.md',
  'docs/design.md',
  'docs/git-workflow.md',
  'docs/library-backend-v0.4.md',
  'docs/live-provider.md',
  'docs/rust-safety-policy.md',
  'docs/rust-dependency-audit.md',
  'docs/rust-host-v0.5.md',
  'docs/spec-v0.1.md',
  'docs/web-backend-v0.2.md',
  'examples/discount/tests.json',
  'examples/discount/v1.ail',
  'examples/discount/v2.ail',
  'examples/libraries/tests.json',
  'examples/libraries/text.ail',
  'examples/tasks/scenarios.json',
  'examples/tasks/service.ail',
  'scripts/bootstrap.ps1',
  'scripts/audit-rust.ps1',
  'scripts/check-rust.ps1',
  'scripts/diff-frontends.ps1',
  'scripts/serve-tasks-rust.ps1',
  'scripts/serve-tasks.ps1',
  'scripts/test.ps1',
  'src/ast.rkt',
  'src/cli.rkt',
  'src/conformance-suite.rkt',
  'src/error.rkt',
  'src/evolution-loop.rkt',
  'src/evolver.rkt',
  'src/http-json.rkt',
  'src/http-server.rkt',
  'src/kv-store.rkt',
  'src/library-backend.rkt',
  'src/library-contract.rkt',
  'src/parser.rkt',
  'src/reader.rkt',
  'src/runtime.rkt',
  'src/schema.rkt',
  'src/service-deployment.rkt',
  'src/service-test-suite.rkt',
  'src/service.rkt',
  'src/test-suite.rkt',
  'src/version-store.rkt',
  'src/version-store-suite.rkt',
  'tests/all.rkt',
  'rust/crates/ail-cli/src/main.rs',
  'rust/crates/ail-conformance/src/lib.rs',
  'rust/crates/ail-diagnostic/src/lib.rs',
  'rust/crates/ail-http/src/lib.rs',
  'rust/crates/ail-provider/src/lib.rs',
  'rust/crates/ail-runtime/src/budget.rs',
  'rust/crates/ail-runtime/src/lib.rs',
  'rust/crates/ail-runtime/src/schema.rs',
  'rust/crates/ail-runtime/src/value.rs',
  'rust/crates/ail-service/src/lib.rs',
  'rust/crates/ail-server/src/main.rs',
  'rust/crates/ail-store/src/lib.rs',
  'rust/crates/ail-store/src/scenario.rs',
  'rust/crates/ail-syntax/src/ast.rs',
  'rust/crates/ail-syntax/src/lib.rs',
  'rust/crates/ail-syntax/src/parser.rs',
  'rust/crates/ail-syntax/src/reader.rs'
]

// The mirror is generated output under wiki/public only. Recreate it so files
// removed from the explicit allowlist cannot linger in a later build.
await rm(outputRoot, { recursive: true, force: true })

await Promise.all(
  publishedFiles.map(async (relativePath) => {
    const source = join(repositoryRoot, relativePath)
    // Appending .txt keeps Markdown snapshots from being interpreted as Wiki
    // pages and makes source files render as plain text in a browser.
    const destination = join(outputRoot, `${relativePath}.txt`)
    await mkdir(dirname(destination), { recursive: true })
    await copyFile(source, destination)
  })
)

console.log(`Synced ${publishedFiles.length} repository files to the Wiki source mirror.`)
