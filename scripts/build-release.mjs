import { spawnSync } from 'node:child_process'
import { mkdtemp, mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import {
  RELEASE_SCHEMA_VERSION,
  RELEASE_TARGETS,
  canonicalJson,
  createDeterministicZip,
  readWorkspaceReleaseMetadata,
  requireGitCommit,
  requireSourceEpoch,
  requireStableVersion,
  sha256
} from './release-lib.mjs'

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

function requiredOption(name) {
  const index = process.argv.indexOf(name)
  const value = index === -1 ? undefined : process.argv[index + 1]
  if (!value || value.startsWith('--')) throw new Error(`${name} requires a value`)
  return value
}

function hasOption(name) {
  return process.argv.includes(name)
}

function run(command, arguments_, options = {}) {
  const result = spawnSync(command, arguments_, {
    cwd: repositoryRoot,
    encoding: options.encoding,
    env: options.env ?? process.env,
    shell: false,
    stdio: options.stdio
  })
  if (result.error) throw result.error
  if (result.status !== (options.expectedStatus ?? 0)) {
    throw new Error(`${command} ${arguments_.join(' ')} exited with status ${result.status}`)
  }
  return result
}

if (process.env.RUSTFLAGS || process.env.CARGO_ENCODED_RUSTFLAGS) {
  throw new Error('release builds reject inherited RUSTFLAGS and CARGO_ENCODED_RUSTFLAGS')
}

const target = requiredOption('--target')
const targetConfiguration = RELEASE_TARGETS[target]
if (!targetConfiguration) throw new Error(`unsupported release target: ${target}`)

const workspace = await readWorkspaceReleaseMetadata(repositoryRoot)
const version = requireStableVersion(requiredOption('--version'))
if (version !== workspace.version) {
  throw new Error(`requested version ${version} does not match workspace version ${workspace.version}`)
}
const sourceCommit = requireGitCommit(requiredOption('--source-commit'))
const sourceDateEpoch = requireSourceEpoch(requiredOption('--source-date-epoch'))
const headCommit = run('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).stdout.trim()
if (headCommit !== sourceCommit) {
  throw new Error(`source commit ${sourceCommit} does not match checked-out HEAD ${headCommit}`)
}
const commitEpoch = requireSourceEpoch(
  run('git', ['show', '-s', '--format=%ct', sourceCommit], { encoding: 'utf8' }).stdout.trim()
)
if (commitEpoch !== sourceDateEpoch) {
  throw new Error(`source date epoch ${sourceDateEpoch} does not match commit epoch ${commitEpoch}`)
}
const trackedStatus = run('git', ['status', '--porcelain'], {
  encoding: 'utf8'
}).stdout.trim()
const sourceTreeClean = trackedStatus.length === 0
if (!sourceTreeClean && !hasOption('--allow-dirty')) {
  throw new Error('release builds require a clean worktree; --allow-dirty is rehearsal-only')
}
const outputDirectory = resolve(repositoryRoot, requiredOption('--out-dir'))
const buildRoot = resolve(repositoryRoot, '.runtime', 'release-build')
await mkdir(outputDirectory, { recursive: true })
await mkdir(buildRoot, { recursive: true })

const firstTargetDirectory = await mkdtemp(join(buildRoot, `${target}-first-`))
const secondTargetDirectory = await mkdtemp(join(buildRoot, `${target}-second-`))
const remapPath = `--remap-path-prefix=${repositoryRoot}=/source/yanshu`
const buildEnvironment = {
  ...process.env,
  CARGO_INCREMENTAL: '0',
  SOURCE_DATE_EPOCH: String(sourceDateEpoch)
}
const cargoArguments = [
  'rustc',
  '--locked',
  '--release',
  '-p',
  'yanshu-cli',
  '--target',
  target
]
const finalRustcArguments = [
  remapPath,
  ...(target === 'x86_64-pc-windows-msvc' ? ['-C', 'link-arg=/Brepro'] : [])
]

for (const targetDirectory of [firstTargetDirectory, secondTargetDirectory]) {
  run('cargo', [...cargoArguments, '--target-dir', targetDirectory, '--', ...finalRustcArguments], {
    env: buildEnvironment,
    stdio: 'inherit'
  })
}

const binaryRelativePath = join(target, 'release', targetConfiguration.binaryName)
const firstBinary = await readFile(join(firstTargetDirectory, binaryRelativePath))
const secondBinary = await readFile(join(secondTargetDirectory, binaryRelativePath))
const firstDigest = sha256(firstBinary)
const secondDigest = sha256(secondBinary)
if (firstDigest !== secondDigest || !firstBinary.equals(secondBinary)) {
  throw new Error(`release binary is not reproducible: ${firstDigest} != ${secondDigest}`)
}

const smoke = run(join(firstTargetDirectory, binaryRelativePath), [], {
  encoding: 'utf8',
  expectedStatus: 1
})
let smokeDocument
try {
  smokeDocument = JSON.parse(smoke.stdout)
} catch {
  throw new Error('release binary smoke test did not return JSON')
}
if (smokeDocument?.error?.code !== 'CLI_USAGE') {
  throw new Error('release binary smoke test did not return CLI_USAGE')
}

const archiveStem = `yanshu-v${version}-${target}`
const archiveName = `${archiveStem}.zip`
const archiveEntries = [
  {
    path: `${archiveStem}/${targetConfiguration.binaryName}`,
    data: firstBinary,
    mode: 0o100755
  },
  {
    path: `${archiveStem}/README.md`,
    data: await readFile(join(repositoryRoot, 'README.md'))
  },
  {
    path: `${archiveStem}/LICENSE-APACHE`,
    data: await readFile(join(repositoryRoot, 'LICENSE-APACHE'))
  },
  {
    path: `${archiveStem}/LICENSE-MIT`,
    data: await readFile(join(repositoryRoot, 'LICENSE-MIT'))
  }
]
const firstArchive = createDeterministicZip(archiveEntries)
const secondArchive = createDeterministicZip([...archiveEntries].reverse())
if (!firstArchive.equals(secondArchive)) {
  throw new Error('release archive changes when input order changes')
}
await writeFile(join(outputDirectory, archiveName), firstArchive, { flag: 'wx' })

const rustc = run('rustc', ['--version'], { encoding: 'utf8' }).stdout.trim()
const cargo = run('cargo', ['--version'], { encoding: 'utf8' }).stdout.trim()
const record = {
  schemaVersion: RELEASE_SCHEMA_VERSION,
  product: 'Yanshu',
  version,
  target,
  targetLabel: targetConfiguration.label,
  sourceCommit,
  sourceDateEpoch,
  sourceTreeClean,
  build: {
    cargo,
    cargoLocked: true,
    profile: 'release',
    repetitions: 2,
    rustc,
    sourcePathRemapped: true,
    windowsBrepro: target === 'x86_64-pc-windows-msvc'
  },
  binary: {
    name: targetConfiguration.binaryName,
    sha256: firstDigest,
    size: firstBinary.length
  },
  archive: {
    name: archiveName,
    sha256: sha256(firstArchive),
    size: firstArchive.length,
    entries: archiveEntries.map((entry) => entry.path).sort()
  }
}
await writeFile(
  join(outputDirectory, `${archiveStem}.build.json`),
  canonicalJson(record),
  { encoding: 'utf8', flag: 'wx' }
)

process.stdout.write(canonicalJson(record))
