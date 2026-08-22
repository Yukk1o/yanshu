import assert from 'node:assert/strict'
import { test } from 'node:test'
import { spawnSync } from 'node:child_process'
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

import {
  RELEASE_EXTENSION,
  RELEASE_PROGRAMS,
  RELEASE_SCHEMA_VERSION,
  RELEASE_SBOM_COMPONENTS,
  RELEASE_TARGETS,
  canonicalJson,
  createDeterministicZip,
  formatChecksums,
  parseChecksums,
  releaseBinaryName,
  releaseSbomRootReference,
  releaseRustToolchain,
  releaseVsixName,
  sha256,
  validateReleaseTag
} from './release-lib.mjs'

const scriptsDirectory = dirname(fileURLToPath(import.meta.url))

test('deterministic ZIP ignores input order and fixes metadata', () => {
  const entries = [
    { path: 'yanshu/bin/yanshu', data: Buffer.from('binary'), mode: 0o100755 },
    { path: 'yanshu/README.md', data: Buffer.from('readme') }
  ]
  const first = createDeterministicZip(entries)
  const second = createDeterministicZip([...entries].reverse())
  assert.deepEqual(first, second)
  assert.equal(sha256(first), sha256(second))
  assert.equal(first.readUInt32LE(0), 0x04034b50)
})

test('deterministic ZIP rejects traversal and duplicate paths', () => {
  assert.throws(
    () => createDeterministicZip([{ path: '../yanshu', data: Buffer.alloc(0) }]),
    /invalid release archive path/
  )
  assert.throws(
    () =>
      createDeterministicZip([
        { path: 'yanshu', data: Buffer.from('a') },
        { path: 'yanshu', data: Buffer.from('b') }
      ]),
    /duplicate release archive path/
  )
})

test('checksum documents are sorted, strict, and round-trip', () => {
  const document = formatChecksums([
    ['z.zip', 'b'.repeat(64)],
    ['a.json', 'a'.repeat(64)]
  ])
  assert.equal(document, `${'a'.repeat(64)}  a.json\n${'b'.repeat(64)}  z.zip\n`)
  assert.deepEqual([...parseChecksums(document)], [
    ['a.json', 'a'.repeat(64)],
    ['z.zip', 'b'.repeat(64)]
  ])
  assert.throws(() => parseChecksums(document.trimEnd()), /must end with LF/)
})

test('release tags exactly bind the workspace version', () => {
  assert.equal(validateReleaseTag('v0.11.0', '0.11.0'), 'v0.11.0')
  assert.throws(() => validateReleaseTag('v0.11', '0.11.0'), /does not match/)
  assert.throws(() => validateReleaseTag('v0.11.0-rc.1', '0.11.0'), /does not match/)
})

test('release toolchains pin the patch component', () => {
  assert.equal(releaseRustToolchain('1.97'), '1.97.0')
  assert.equal(releaseRustToolchain('1.97.1'), '1.97.1')
  assert.throws(() => releaseRustToolchain('stable'), /not a release toolchain version/)
})

test('release assembly closes programs, extensions, and every payload hash', async (context) => {
  const temporaryRoot = await mkdtemp(join(tmpdir(), 'yanshu-release-test-'))
  context.after(() => rm(temporaryRoot, { recursive: true, force: true }))
  const inputDirectory = join(temporaryRoot, 'input')
  const outputDirectory = join(temporaryRoot, 'output')
  await mkdir(inputDirectory)
  const version = '0.11.0'
  const sourceCommit = 'a'.repeat(40)
  const sourceDateEpoch = 1_700_000_000

  for (const [target, configuration] of Object.entries(RELEASE_TARGETS)) {
    const stem = `yanshu-v${version}-${target}`
    const archiveName = `${stem}.zip`
    const archive = Buffer.from(`archive:${target}`)
    const vsixName = releaseVsixName(version, configuration)
    const vsix = Buffer.from(`vsix:${target}`)
    await writeFile(join(inputDirectory, archiveName), archive)
    await writeFile(join(inputDirectory, vsixName), vsix)
    await writeFile(
      join(inputDirectory, `${stem}.build.json`),
      canonicalJson({
        schemaVersion: RELEASE_SCHEMA_VERSION,
        product: 'Yanshu',
        version,
        target,
        sourceCommit,
        sourceDateEpoch,
        sourceTreeClean: true,
        build: {
          cargoLocked: true,
          node: 'v22.0.0',
          repetitions: 2,
          vsce: '3.9.2'
        },
        binaries: RELEASE_PROGRAMS.map((program) => ({
          key: program.key,
          package: program.packageName,
          name: releaseBinaryName(program, configuration),
          sha256: 'b'.repeat(64),
          size: 1,
          smokeTest: program.smokeTest
        })),
        archive: {
          name: archiveName,
          sha256: sha256(archive),
          size: archive.length,
          entries: [
            ...RELEASE_PROGRAMS.map(
              (program) => `${stem}/${releaseBinaryName(program, configuration)}`
            ),
            `${stem}/README.md`,
            `${stem}/LICENSE-APACHE`,
            `${stem}/LICENSE-MIT`
          ].sort((left, right) => left.localeCompare(right, 'en'))
        },
        extension: {
          bundledBinary: 'lsp',
          bundledBinarySha256: 'b'.repeat(64),
          name: vsixName,
          package: RELEASE_EXTENSION.packageName,
          repetitions: 2,
          sha256: sha256(vsix),
          size: vsix.length,
          smokeTest: RELEASE_EXTENSION.smokeTest,
          target: configuration.vsixTarget
        }
      })
    )
  }

  for (const program of RELEASE_SBOM_COMPONENTS) {
    await writeFile(
      join(inputDirectory, `yanshu-v${version}-${program.key}.cdx.json`),
      canonicalJson({
        bomFormat: 'CycloneDX',
        specVersion: '1.5',
        metadata: {
          component: {
            'bom-ref': releaseSbomRootReference(program, version, sourceCommit),
            version
          },
          properties: [
            ...(program === RELEASE_EXTENSION
              ? [{ name: 'yanshu:bundled-program', value: 'lsp' }]
              : []),
            { name: 'yanshu:source-commit', value: sourceCommit },
            { name: 'yanshu:release-program', value: program.key }
          ]
        }
      })
    )
  }

  const assemble = spawnSync(
    process.execPath,
    [
      join(scriptsDirectory, 'assemble-release.mjs'),
      '--input-dir',
      inputDirectory,
      '--output-dir',
      outputDirectory,
      '--version',
      version,
      '--source-commit',
      sourceCommit,
      '--source-date-epoch',
      String(sourceDateEpoch)
    ],
    { encoding: 'utf8', shell: false }
  )
  assert.equal(assemble.status, 0, assemble.stderr)

  const verifyArguments = [join(scriptsDirectory, 'verify-release.mjs'), outputDirectory]
  const verified = spawnSync(process.execPath, verifyArguments, { encoding: 'utf8', shell: false })
  assert.equal(verified.status, 0, verified.stderr)
  const manifest = JSON.parse(
    await readFile(join(outputDirectory, `yanshu-v${version}.release.json`), 'utf8')
  )
  assert.deepEqual(
    manifest.sboms.map((sbom) => sbom.program),
    RELEASE_SBOM_COMPONENTS.map((program) => program.key)
  )

  const firstTarget = Object.keys(RELEASE_TARGETS).sort()[0]
  const tamperedArchive = join(outputDirectory, `yanshu-v${version}-${firstTarget}.zip`)
  const originalArchive = await readFile(tamperedArchive)
  await writeFile(tamperedArchive, Buffer.concat([originalArchive, Buffer.from('!')]))
  const rejected = spawnSync(process.execPath, verifyArguments, { encoding: 'utf8', shell: false })
  assert.notEqual(rejected.status, 0)
  assert.match(rejected.stderr, /checksum mismatch/)

  await writeFile(tamperedArchive, originalArchive)
  const mcpSbomName = `yanshu-v${version}-mcp.cdx.json`
  const mcpSbomPath = join(outputDirectory, mcpSbomName)
  const mcpSbom = JSON.parse(await readFile(mcpSbomPath, 'utf8'))
  mcpSbom.metadata.component['bom-ref'] =
    `urn:yanshu:crate:yanshu-cli:${version}:${sourceCommit}`
  const tamperedSbom = Buffer.from(canonicalJson(mcpSbom))
  await writeFile(mcpSbomPath, tamperedSbom)
  const mcpAsset = manifest.assets.find((asset) => asset.name === mcpSbomName)
  mcpAsset.sha256 = sha256(tamperedSbom)
  mcpAsset.size = tamperedSbom.length
  const manifestName = `yanshu-v${version}.release.json`
  const manifestPath = join(outputDirectory, manifestName)
  await writeFile(manifestPath, canonicalJson(manifest))
  const checksumNames = [...manifest.assets.map((asset) => asset.name), manifestName]
  const checksumEntries = await Promise.all(
    checksumNames.map(async (name) => [name, sha256(await readFile(join(outputDirectory, name)))])
  )
  await writeFile(join(outputDirectory, 'SHA256SUMS'), formatChecksums(checksumEntries))

  const mislabeled = spawnSync(process.execPath, verifyArguments, {
    encoding: 'utf8',
    shell: false
  })
  assert.notEqual(mislabeled.status, 0)
  assert.match(mislabeled.stderr, /release SBOM does not describe mcp/)
})

test('SBOM normalization removes random and checkout-local identity', async (context) => {
  const temporaryRoot = await mkdtemp(join(tmpdir(), 'yanshu-sbom-test-'))
  context.after(() => rm(temporaryRoot, { recursive: true, force: true }))
  const input = join(temporaryRoot, 'input.json')
  const output = join(temporaryRoot, 'output.json')
  const sourceCommit = 'c'.repeat(40)
  const localReference = 'path+file:///D:/work/yanshu/rust/crates/yanshu-cli#0.11.0'
  await writeFile(
    input,
    canonicalJson({
      bomFormat: 'CycloneDX',
      specVersion: '1.5',
      serialNumber: 'urn:uuid:random',
      metadata: {
        timestamp: '2099-01-01T00:00:00Z',
        component: {
          type: 'application',
          'bom-ref': localReference,
          name: 'yanshu',
          version: '0.11.0',
          purl: 'pkg:cargo/yanshu-cli@0.11.0?download_url=file://.#src/main.rs'
        }
      },
      dependencies: [{ ref: localReference, dependsOn: [] }]
    })
  )
  const normalized = spawnSync(
    process.execPath,
    [
      join(scriptsDirectory, 'normalize-sbom.mjs'),
      '--input',
      input,
      '--output',
      output,
      '--program',
      'cli',
      '--version',
      '0.11.0',
      '--source-commit',
      sourceCommit,
      '--source-date-epoch',
      '1700000000'
    ],
    { encoding: 'utf8', shell: false }
  )
  assert.equal(normalized.status, 0, normalized.stderr)
  const document = await readFile(output, 'utf8')
  const sbom = JSON.parse(document)
  assert.equal(sbom.serialNumber, undefined)
  assert.equal(sbom.metadata.timestamp, '2023-11-14T22:13:20.000Z')
  assert.match(sbom.metadata.component['bom-ref'], /^urn:yanshu:crate:yanshu-cli:/)
  assert.equal(sbom.dependencies[0].ref, sbom.metadata.component['bom-ref'])
  assert.ok(
    sbom.metadata.properties.some(
      (property) => property.name === 'yanshu:release-program' && property.value === 'cli'
    )
  )
  assert.doesNotMatch(document, /path\+file|download_url=file|D:\//)
})

test('extension SBOM normalization binds the npm graph and bundled LSP', async (context) => {
  const temporaryRoot = await mkdtemp(join(tmpdir(), 'yanshu-extension-sbom-test-'))
  context.after(() => rm(temporaryRoot, { recursive: true, force: true }))
  const input = join(temporaryRoot, 'input.json')
  const output = join(temporaryRoot, 'output.json')
  const version = '0.11.0'
  const sourceCommit = 'd'.repeat(40)
  const localReference = `${RELEASE_EXTENSION.packageName}@${version}`
  await writeFile(
    input,
    canonicalJson({
      bomFormat: 'CycloneDX',
      specVersion: '1.5',
      serialNumber: 'urn:uuid:random',
      metadata: {
        timestamp: '2099-01-01T00:00:00Z',
        component: {
          'bom-ref': localReference,
          name: 'vscode',
          purl: `pkg:npm/${RELEASE_EXTENSION.packageName}@${version}`,
          type: 'application',
          version
        },
        properties: [{ name: 'cdx:npm:package:path', value: '' }]
      },
      components: [
        {
          'bom-ref': 'vscode-jsonrpc@9.0.1',
          name: 'vscode-jsonrpc',
          purl: 'pkg:npm/vscode-jsonrpc@9.0.1',
          type: 'library',
          version: '9.0.1'
        }
      ],
      dependencies: [{ ref: localReference, dependsOn: ['vscode-jsonrpc@9.0.1'] }]
    })
  )

  const normalized = spawnSync(
    process.execPath,
    [
      join(scriptsDirectory, 'normalize-extension-sbom.mjs'),
      '--input',
      input,
      '--output',
      output,
      '--version',
      version,
      '--source-commit',
      sourceCommit,
      '--source-date-epoch',
      '1700000000'
    ],
    { encoding: 'utf8', shell: false }
  )
  assert.equal(normalized.status, 0, normalized.stderr)

  const document = await readFile(output, 'utf8')
  const sbom = JSON.parse(document)
  const expectedRoot = releaseSbomRootReference(RELEASE_EXTENSION, version, sourceCommit)
  assert.equal(sbom.serialNumber, undefined)
  assert.equal(sbom.metadata.timestamp, '2023-11-14T22:13:20.000Z')
  assert.equal(sbom.metadata.component.name, RELEASE_EXTENSION.packageName)
  assert.equal(sbom.metadata.component['bom-ref'], expectedRoot)
  assert.equal(sbom.dependencies[0].ref, expectedRoot)
  assert.ok(
    sbom.metadata.properties.some(
      (property) => property.name === 'yanshu:bundled-program' && property.value === 'lsp'
    )
  )
  assert.ok(
    sbom.metadata.properties.some(
      (property) =>
        property.name === 'yanshu:release-program' &&
        property.value === RELEASE_EXTENSION.key
    )
  )
  assert.doesNotMatch(document, /2099-01-01|urn:uuid:random/)
})
