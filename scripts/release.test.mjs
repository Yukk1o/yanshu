import assert from 'node:assert/strict'
import { test } from 'node:test'
import { spawnSync } from 'node:child_process'
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

import {
  RELEASE_SCHEMA_VERSION,
  RELEASE_TARGETS,
  canonicalJson,
  createDeterministicZip,
  formatChecksums,
  parseChecksums,
  releaseRustToolchain,
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

test('release assembly closes and verifies every payload hash', async (context) => {
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
    await writeFile(join(inputDirectory, archiveName), archive)
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
        build: { cargoLocked: true, repetitions: 2 },
        binary: { name: configuration.binaryName, sha256: 'b'.repeat(64), size: 1 },
        archive: { name: archiveName, sha256: sha256(archive), size: archive.length }
      })
    )
  }

  await writeFile(
    join(inputDirectory, `yanshu-v${version}.cdx.json`),
    canonicalJson({
      bomFormat: 'CycloneDX',
      specVersion: '1.5',
      metadata: {
        component: { version },
        properties: [{ name: 'yanshu:source-commit', value: sourceCommit }]
      }
    })
  )

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

  const firstTarget = Object.keys(RELEASE_TARGETS).sort()[0]
  const tamperedArchive = join(outputDirectory, `yanshu-v${version}-${firstTarget}.zip`)
  await writeFile(tamperedArchive, Buffer.concat([await readFile(tamperedArchive), Buffer.from('!')]))
  const rejected = spawnSync(process.execPath, verifyArguments, { encoding: 'utf8', shell: false })
  assert.notEqual(rejected.status, 0)
  assert.match(rejected.stderr, /checksum mismatch/)
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
  assert.doesNotMatch(document, /path\+file|download_url=file|D:\//)
})
