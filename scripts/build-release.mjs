import { spawnSync } from 'node:child_process'
import { mkdtemp, mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import {
  RELEASE_EXTENSION,
  RELEASE_PROGRAMS,
  RELEASE_SCHEMA_VERSION,
  RELEASE_TARGETS,
  canonicalJson,
  createDeterministicZip,
  readWorkspaceReleaseMetadata,
  releaseBinaryName,
  releaseVsixName,
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
    cwd: options.cwd ?? repositoryRoot,
    encoding: options.encoding,
    env: options.env ?? process.env,
    input: options.input,
    maxBuffer: options.maxBuffer,
    shell: false,
    stdio: options.stdio,
    timeout: options.timeout
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
const finalRustcArguments = [
  remapPath,
  ...(target === 'x86_64-pc-windows-msvc' ? ['-C', 'link-arg=/Brepro'] : [])
]

for (const targetDirectory of [firstTargetDirectory, secondTargetDirectory]) {
  for (const program of RELEASE_PROGRAMS) {
    run(
      'cargo',
      [
        'rustc',
        '--locked',
        '--release',
        '-p',
        program.packageName,
        '--bin',
        program.binaryStem,
        '--target',
        target,
        '--target-dir',
        targetDirectory,
        '--',
        ...finalRustcArguments
      ],
      {
        env: buildEnvironment,
        stdio: 'inherit'
      }
    )
  }
}

function smokeCli(binaryPath) {
  const smoke = run(binaryPath, [], {
    encoding: 'utf8',
    expectedStatus: 1,
    maxBuffer: 64 * 1024,
    timeout: 5_000
  })
  let smokeDocument
  try {
    smokeDocument = JSON.parse(smoke.stdout)
  } catch {
    throw new Error('release CLI smoke test did not return JSON')
  }
  if (smokeDocument?.error?.code !== 'CLI_USAGE') {
    throw new Error('release CLI smoke test did not return CLI_USAGE')
  }
}

function smokeMcp(binaryPath) {
  const input = [
    {
      jsonrpc: '2.0',
      id: 1,
      method: 'initialize',
      params: {
        protocolVersion: '2025-11-25',
        capabilities: {},
        clientInfo: { name: 'release-smoke', version: '1' }
      }
    },
    { jsonrpc: '2.0', id: 2, method: 'tools/list', params: {} }
  ]
    .map((message) => JSON.stringify(message))
    .join('\n') + '\n'
  const smoke = run(binaryPath, [], {
    encoding: 'utf8',
    expectedStatus: 0,
    input,
    maxBuffer: 64 * 1024,
    timeout: 5_000
  })
  const lines = smoke.stdout.trimEnd().split('\n')
  if (smoke.stderr !== '' || lines.length !== 2) {
    throw new Error('release MCP smoke test did not return exactly two clean JSON-RPC responses')
  }
  let responses
  try {
    responses = lines.map((line) => JSON.parse(line))
  } catch {
    throw new Error('release MCP smoke test did not return newline-delimited JSON')
  }
  const toolNames = responses[1]?.result?.tools?.map((tool) => tool.name)
  if (
    responses[0]?.jsonrpc !== '2.0' ||
    responses[0]?.id !== 1 ||
    responses[1]?.jsonrpc !== '2.0' ||
    responses[1]?.id !== 2 ||
    responses[0]?.result?.serverInfo?.name !== 'yanshu-mcp' ||
    JSON.stringify(toolNames) !==
      JSON.stringify(['yanshu.inspect_source', 'yanshu.format_source', 'yanshu.review_source'])
  ) {
    throw new Error('release MCP smoke test did not expose the expected read-only tool set')
  }
}

function lspFrame(message) {
  const body = Buffer.from(JSON.stringify(message), 'utf8')
  return Buffer.concat([
    Buffer.from(`Content-Length: ${body.length}`, 'ascii'),
    Buffer.from([13, 10, 13, 10]),
    body
  ])
}

function parseLspResponses(output) {
  const responses = []
  let offset = 0
  while (offset < output.length) {
    const headerEnd = output.indexOf(Buffer.from([13, 10, 13, 10]), offset)
    if (headerEnd === -1) {
      throw new Error('release LSP smoke returned an incomplete header')
    }
    const header = output.subarray(offset, headerEnd).toString('ascii')
    const match = header.match(/^Content-Length: ([0-9]+)$/)
    if (!match) {
      throw new Error('release LSP smoke returned unexpected framing headers')
    }
    const length = Number(match[1])
    if (!Number.isSafeInteger(length) || length <= 0 || length > 64 * 1024) {
      throw new Error('release LSP smoke response length is outside bounds')
    }
    const bodyStart = headerEnd + 4
    const bodyEnd = bodyStart + length
    if (bodyEnd > output.length) {
      throw new Error('release LSP smoke returned an incomplete body')
    }
    try {
      responses.push(JSON.parse(output.subarray(bodyStart, bodyEnd).toString('utf8')))
    } catch {
      throw new Error('release LSP smoke did not return JSON-RPC JSON')
    }
    offset = bodyEnd
  }
  return responses
}

function smokeLsp(binaryPath) {
  const input = Buffer.concat([
    lspFrame({ jsonrpc: '2.0', id: 1, method: 'initialize', params: {} }),
    lspFrame({ jsonrpc: '2.0', id: 2, method: 'shutdown' }),
    lspFrame({ jsonrpc: '2.0', method: 'exit' })
  ])
  const smoke = run(binaryPath, [], {
    expectedStatus: 0,
    input,
    maxBuffer: 64 * 1024,
    timeout: 5_000
  })
  if (smoke.stderr.length !== 0) {
    throw new Error('release LSP smoke wrote unexpected stderr')
  }
  const responses = parseLspResponses(smoke.stdout)
  if (
    responses.length !== 2 ||
    responses[0]?.jsonrpc !== '2.0' ||
    responses[0]?.id !== 1 ||
    responses[0]?.result?.serverInfo?.name !== 'yanshu-lsp' ||
    responses[0]?.result?.capabilities?.positionEncoding !== 'utf-16' ||
    responses[0]?.result?.capabilities?.experimental?.yanshuReviewDocument?.editable !== false ||
    responses[1]?.jsonrpc !== '2.0' ||
    responses[1]?.id !== 2 ||
    responses[1]?.result !== null
  ) {
    throw new Error('release LSP smoke did not return the expected read-only capability contract')
  }
}

const builtPrograms = []
for (const program of RELEASE_PROGRAMS) {
  const binaryName = releaseBinaryName(program, targetConfiguration)
  const binaryRelativePath = join(target, 'release', binaryName)
  const firstBinary = await readFile(join(firstTargetDirectory, binaryRelativePath))
  const secondBinary = await readFile(join(secondTargetDirectory, binaryRelativePath))
  const firstDigest = sha256(firstBinary)
  const secondDigest = sha256(secondBinary)
  if (firstDigest !== secondDigest || !firstBinary.equals(secondBinary)) {
    throw new Error(
      `release binary ${program.key} is not reproducible: ${firstDigest} != ${secondDigest}`
    )
  }
  const binaryPath = join(firstTargetDirectory, binaryRelativePath)
  if (program.key === 'cli') smokeCli(binaryPath)
  else if (program.key === 'mcp') smokeMcp(binaryPath)
  else if (program.key === 'lsp') smokeLsp(binaryPath)
  else throw new Error(`release program has no smoke test: ${program.key}`)
  builtPrograms.push({
    data: firstBinary,
    record: {
      key: program.key,
      package: program.packageName,
      name: binaryName,
      sha256: firstDigest,
      size: firstBinary.length,
      smokeTest: program.smokeTest
    }
  })
}

const lspProgram = RELEASE_PROGRAMS.find((program) => program.key === 'lsp')
if (!lspProgram) throw new Error('release programs do not declare the LSP')
const lspBinaryName = releaseBinaryName(lspProgram, targetConfiguration)
const lspRelativePath = join(target, 'release', lspBinaryName)
const extensionRoot = join(repositoryRoot, 'editors', 'vscode')
const vsixName = releaseVsixName(version, targetConfiguration)
const packagedVsixPath = join(extensionRoot, 'dist', vsixName)

function packageVsix(lspBinaryPath) {
  run(process.execPath, [join(extensionRoot, 'scripts', 'package.cjs')], {
    env: {
      ...buildEnvironment,
      YANSHU_LSP_BINARY: lspBinaryPath
    },
    stdio: 'inherit'
  })
}

packageVsix(join(firstTargetDirectory, lspRelativePath))
const firstVsix = await readFile(packagedVsixPath)
packageVsix(join(secondTargetDirectory, lspRelativePath))
const secondVsix = await readFile(packagedVsixPath)
const firstVsixDigest = sha256(firstVsix)
const secondVsixDigest = sha256(secondVsix)
if (firstVsixDigest !== secondVsixDigest || !firstVsix.equals(secondVsix)) {
  throw new Error(`release VSIX is not reproducible: ${firstVsixDigest} != ${secondVsixDigest}`)
}
await writeFile(join(outputDirectory, vsixName), firstVsix, { flag: 'wx' })

const archiveStem = `yanshu-v${version}-${target}`
const archiveName = `${archiveStem}.zip`
const archiveEntries = [
  ...builtPrograms.map((program) => ({
    path: `${archiveStem}/${program.record.name}`,
    data: program.data,
    mode: 0o100755
  })),
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
const node = process.version
const lspRecord = builtPrograms.find((program) => program.record.key === 'lsp')?.record
if (!lspRecord) throw new Error('release build did not record the LSP')
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
    node,
    profile: 'release',
    repetitions: 2,
    rustc,
    sourcePathRemapped: true,
    vsce: workspace.vsceVersion,
    windowsBrepro: target === 'x86_64-pc-windows-msvc'
  },
  binaries: builtPrograms.map((program) => program.record),
  extension: {
    bundledBinary: lspRecord.key,
    bundledBinarySha256: lspRecord.sha256,
    name: vsixName,
    package: RELEASE_EXTENSION.packageName,
    repetitions: 2,
    sha256: firstVsixDigest,
    size: firstVsix.length,
    smokeTest: RELEASE_EXTENSION.smokeTest,
    target: targetConfiguration.vsixTarget
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
