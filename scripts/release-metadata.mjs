import { appendFile, mkdir, writeFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'

import {
  RELEASE_SCHEMA_VERSION,
  canonicalJson,
  readWorkspaceReleaseMetadata,
  releaseRustToolchain,
  requireGitCommit,
  requireSourceEpoch,
  validateReleaseTag
} from './release-lib.mjs'

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

function option(name) {
  const index = process.argv.indexOf(name)
  if (index === -1) return undefined
  const value = process.argv[index + 1]
  if (!value || value.startsWith('--')) throw new Error(`${name} requires a value`)
  return value
}

function git(...arguments_) {
  const result = spawnSync('git', arguments_, {
    cwd: repositoryRoot,
    encoding: 'utf8',
    shell: false
  })
  if (result.status !== 0) {
    throw new Error(`git ${arguments_.join(' ')} failed: ${result.stderr.trim()}`)
  }
  return result.stdout.trim()
}

const workspace = await readWorkspaceReleaseMetadata(repositoryRoot)
const tag = option('--tag')
if (tag) validateReleaseTag(tag, workspace.version)

const sourceCommit = requireGitCommit(option('--source-commit') ?? git('rev-parse', 'HEAD'))
const sourceDateEpoch = requireSourceEpoch(
  option('--source-date-epoch') ?? git('show', '-s', '--format=%ct', sourceCommit)
)
const metadata = {
  schemaVersion: RELEASE_SCHEMA_VERSION,
  product: 'Yanshu',
  version: workspace.version,
  tag: tag ?? null,
  sourceCommit,
  sourceDateEpoch,
  rustVersion: workspace.rustVersion,
  rustToolchain: releaseRustToolchain(workspace.rustVersion),
  firstPartyCrates: workspace.crateCount
}
const document = canonicalJson(metadata)

const output = option('--output')
if (output) {
  const destination = resolve(repositoryRoot, output)
  await mkdir(dirname(destination), { recursive: true })
  await writeFile(destination, document, { encoding: 'utf8', flag: 'wx' })
}

const githubOutput = option('--github-output')
if (githubOutput) {
  await appendFile(
    githubOutput,
    [
      `version=${workspace.version}`,
      `source_commit=${sourceCommit}`,
      `source_date_epoch=${sourceDateEpoch}`,
      `rust_version=${workspace.rustVersion}`,
      `rust_toolchain=${releaseRustToolchain(workspace.rustVersion)}`,
      ''
    ].join('\n'),
    'utf8'
  )
}

process.stdout.write(document)
