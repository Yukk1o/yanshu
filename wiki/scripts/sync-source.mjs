import { copyFile, mkdir, rm } from 'node:fs/promises'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const wikiRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repositoryRoot = resolve(wikiRoot, '..')
const outputRoot = join(wikiRoot, 'public', 'source')

// These are snapshots of files linked by the language Wiki. Keep the list
// explicit so the build cannot publish credentials, runtime data, or unrelated
// implementation history.
const publishedFiles = [
  'Cargo.lock',
  'Cargo.toml',
  'deny.toml',
  'conformance/v1/invalid/multiple-forms.ail',
  'conformance/v1/invalid/unknown-library.ail',
  'conformance/v1/manifest.json',
  'conformance/v1/programs/core.ail',
  'conformance/v1/programs/library.ail',
  'conformance/v1/programs/schema.ail',
  'docs/rust-safety-policy.md',
  'docs/rust-dependency-audit.md',
  'docs/git-workflow.md',
  'docs/backup-restore.md',
  'docs/shadow-rollout.md',
  'docs/spec-v0.1.md',
  'docs/spec-v0.6.md',
  'docs/spec-v0.7.md',
  'docs/spec-v0.8.md',
  'docs/spec-v0.9.md',
  'docs/spec-v0.10.md',
  'docs/ai-agent-guide.md',
  'examples/discount/tests.json',
  'examples/discount/v1.ail',
  'examples/discount/v2.ail',
  'examples/libraries/tests.json',
  'examples/libraries/text.ail',
  'examples/tasks/scenarios.json',
  'examples/tasks/service.ail',
  'examples/expenses/scenarios.json',
  'examples/expenses/service.ail',
  'examples/bundles/expense-approval/app.ail',
  'examples/bundles/expense-approval/policy.ail',
  'examples/bundles/expense-approval/bundle.json',
  'examples/bundles/typed-expense/app.ail',
  'examples/bundles/typed-expense/policy.ail',
  'examples/bundles/typed-expense/bundle.json',
  'examples/packages/typed-expense/ail-package.source.json',
  'examples/packages/typed-expense/ail.lock.json',
  'examples/packages/typed-expense/app.ail',
  'examples/packages/typed-expense/packages/typed-policy/ail-package.source.json',
  'examples/packages/typed-expense/packages/typed-policy/policy.ail',
  'scripts/audit-rust.ps1',
  'scripts/serve-tasks-rust.ps1',
  'rust/crates/ail-cli/src/main.rs',
  'rust/crates/ail-bundle/src/graph.rs',
  'rust/crates/ail-bundle/src/lib.rs',
  'rust/crates/ail-bundle/src/linker.rs',
  'rust/crates/ail-bundle/src/manifest.rs',
  'rust/crates/ail-analysis/src/effects.rs',
  'rust/crates/ail-analysis/src/infer.rs',
  'rust/crates/ail-analysis/src/lib.rs',
  'rust/crates/ail-analysis/src/review.rs',
  'rust/crates/ail-analysis/src/types.rs',
  'rust/crates/ail-compiler/src/artifact.rs',
  'rust/crates/ail-compiler/src/bytecode.rs',
  'rust/crates/ail-compiler/src/compile.rs',
  'rust/crates/ail-compiler/src/lib.rs',
  'rust/crates/ail-compiler/src/verify.rs',
  'rust/crates/ail-compiler/src/wasm.rs',
  'rust/crates/ail-library/src/contract.rs',
  'rust/crates/ail-library/src/lib.rs',
  'rust/crates/ail-library/src/registry.rs',
  'rust/crates/ail-library/src/text.rs',
  'rust/crates/ail-library/src/value.rs',
  'rust/crates/ail-package/src/format.rs',
  'rust/crates/ail-package/src/lib.rs',
  'rust/crates/ail-package/src/model.rs',
  'rust/crates/ail-package/src/parse.rs',
  'rust/crates/ail-package/src/store.rs',
  'rust/crates/ail-conformance/src/lib.rs',
  'rust/crates/ail-diagnostic/src/lib.rs',
  'rust/crates/ail-http/src/lib.rs',
  'rust/crates/ail-http/src/shadow.rs',
  'rust/crates/ail-ops/src/lib.rs',
  'rust/crates/ail-ops/src/lease.rs',
  'rust/crates/ail-ops/src/manifest.rs',
  'rust/crates/ail-ops/src/operations.rs',
  'rust/crates/ail-provider/src/lib.rs',
  'rust/crates/ail-provider/src/agent.rs',
  'rust/crates/ail-rollout/src/comparison.rs',
  'rust/crates/ail-rollout/src/lib.rs',
  'rust/crates/ail-rollout/src/observation.rs',
  'rust/crates/ail-rollout/src/policy.rs',
  'rust/crates/ail-rollout/src/runtime.rs',
  'rust/crates/ail-runtime/src/budget.rs',
  'rust/crates/ail-runtime/src/compiled.rs',
  'rust/crates/ail-runtime/src/lib.rs',
  'rust/crates/ail-runtime/src/matcher.rs',
  'rust/crates/ail-runtime/src/schema.rs',
  'rust/crates/ail-runtime/src/value.rs',
  'rust/crates/ail-service/src/lib.rs',
  'rust/crates/ail-server/src/main.rs',
  'rust/crates/ail-server/src/configuration.rs',
  'rust/crates/ail-store/src/lib.rs',
  'rust/crates/ail-store/src/scenario.rs',
  'rust/crates/ail-syntax/src/ast.rs',
  'rust/crates/ail-syntax/src/lib.rs',
  'rust/crates/ail-syntax/src/parser.rs',
  'rust/crates/ail-syntax/src/reader.rs'
]

// Recreate the generated mirror so removed allowlist entries cannot linger.
await rm(outputRoot, { recursive: true, force: true })

await Promise.all(
  publishedFiles.map(async (relativePath) => {
    const source = join(repositoryRoot, relativePath)
    const destination = join(outputRoot, `${relativePath}.txt`)
    await mkdir(dirname(destination), { recursive: true })
    await copyFile(source, destination)
  })
)

console.log(`Synced ${publishedFiles.length} repository files to the Wiki source mirror.`)
