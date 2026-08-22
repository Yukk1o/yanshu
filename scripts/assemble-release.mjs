import { copyFile, lstat, mkdir, readFile, readdir, writeFile } from 'node:fs/promises'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import {
  RELEASE_PROGRAMS,
  RELEASE_SCHEMA_VERSION,
  RELEASE_TARGETS,
  canonicalJson,
  formatChecksums,
  releaseBinaryName,
  requireGitCommit,
  requireSha256,
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

async function regularFiles(directory) {
  const entries = await readdir(directory, { withFileTypes: true })
  const files = []
  for (const entry of entries) {
    if (!entry.isFile() || entry.isSymbolicLink()) {
      throw new Error(`release input must contain only regular files: ${entry.name}`)
    }
    if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(entry.name)) {
      throw new Error(`release input has an unsafe file name: ${entry.name}`)
    }
    files.push(entry.name)
  }
  return files.sort((left, right) => left.localeCompare(right, 'en'))
}

const inputDirectory = resolve(repositoryRoot, requiredOption('--input-dir'))
const outputDirectory = resolve(repositoryRoot, requiredOption('--output-dir'))
const version = requireStableVersion(requiredOption('--version'))
const sourceCommit = requireGitCommit(requiredOption('--source-commit'))
const sourceDateEpoch = requireSourceEpoch(requiredOption('--source-date-epoch'))
const inputFiles = await regularFiles(inputDirectory)
const expectedInputs = new Set()
const buildRecords = []

for (const target of Object.keys(RELEASE_TARGETS).sort()) {
  const stem = `yanshu-v${version}-${target}`
  const archiveName = `${stem}.zip`
  const recordName = `${stem}.build.json`
  expectedInputs.add(archiveName)
  expectedInputs.add(recordName)

  const record = JSON.parse(await readFile(join(inputDirectory, recordName), 'utf8'))
  const expectedBinaries = RELEASE_PROGRAMS.map((program) => ({
    key: program.key,
    package: program.packageName,
    name: releaseBinaryName(program, RELEASE_TARGETS[target]),
    smokeTest: program.smokeTest
  }))
  const expectedArchiveEntries = [
    ...expectedBinaries.map((binary) => `${stem}/${binary.name}`),
    `${stem}/README.md`,
    `${stem}/LICENSE-APACHE`,
    `${stem}/LICENSE-MIT`
  ].sort((left, right) => left.localeCompare(right, 'en'))
  if (
    record.schemaVersion !== RELEASE_SCHEMA_VERSION ||
    record.product !== 'Yanshu' ||
    record.version !== version ||
    record.target !== target ||
    record.sourceCommit !== sourceCommit ||
    record.sourceDateEpoch !== sourceDateEpoch ||
    record.sourceTreeClean !== true ||
    record.build?.cargoLocked !== true ||
    record.build?.repetitions !== 2 ||
    record.archive?.name !== archiveName ||
    !Array.isArray(record.binaries) ||
    record.binaries.length !== expectedBinaries.length ||
    !Array.isArray(record.archive?.entries) ||
    JSON.stringify(record.archive.entries) !== JSON.stringify(expectedArchiveEntries)
  ) {
    throw new Error(`invalid or mismatched build record: ${recordName}`)
  }
  for (const expectedBinary of expectedBinaries) {
    const binary = record.binaries.find((candidate) => candidate.key === expectedBinary.key)
    if (
      binary?.package !== expectedBinary.package ||
      binary.name !== expectedBinary.name ||
      binary.smokeTest !== expectedBinary.smokeTest ||
      !Number.isSafeInteger(binary.size) ||
      binary.size <= 0
    ) {
      throw new Error(`invalid ${expectedBinary.key} binary record: ${recordName}`)
    }
    requireSha256(binary.sha256, `${expectedBinary.key} binary digest`)
  }
  const archive = await readFile(join(inputDirectory, archiveName))
  if (record.archive.size !== archive.length || record.archive.sha256 !== sha256(archive)) {
    throw new Error(`archive does not match build record: ${archiveName}`)
  }
  buildRecords.push(record)
}

const sbomRecords = []
for (const program of RELEASE_PROGRAMS) {
  const sbomName = `yanshu-v${version}-${program.key}.cdx.json`
  expectedInputs.add(sbomName)
  const sbom = JSON.parse(await readFile(join(inputDirectory, sbomName), 'utf8'))
  const properties = sbom.metadata?.properties ?? []
  const sourceCommitProperties = properties.filter(
    (property) => property.name === 'yanshu:source-commit'
  )
  const releaseProgramProperties = properties.filter(
    (property) => property.name === 'yanshu:release-program'
  )
  const expectedRootReference =
    `urn:yanshu:crate:${program.packageName}:${version}:${sourceCommit}`
  if (
    sbom.bomFormat !== 'CycloneDX' ||
    sbom.specVersion !== '1.5' ||
    sbom.metadata?.component?.version !== version ||
    sbom.metadata.component['bom-ref'] !== expectedRootReference ||
    sourceCommitProperties.length !== 1 ||
    sourceCommitProperties[0].value !== sourceCommit ||
    releaseProgramProperties.length !== 1 ||
    releaseProgramProperties[0].value !== program.key
  ) {
    throw new Error(`normalized SBOM does not describe release program ${program.key}`)
  }
  sbomRecords.push({
    program: program.key,
    package: program.packageName,
    format: 'CycloneDX',
    specVersion: '1.5',
    file: sbomName
  })
}

if (
  inputFiles.length !== expectedInputs.size ||
  inputFiles.some((name) => !expectedInputs.has(name))
) {
  const unexpected = inputFiles.filter((name) => !expectedInputs.has(name))
  const missing = [...expectedInputs].filter((name) => !inputFiles.includes(name))
  throw new Error(`release input mismatch; missing=[${missing}], unexpected=[${unexpected}]`)
}

await mkdir(outputDirectory, { recursive: true })
const existingOutputs = await readdir(outputDirectory)
if (existingOutputs.length !== 0) {
  throw new Error('release output directory must be empty')
}

for (const name of inputFiles) {
  const source = join(inputDirectory, name)
  const status = await lstat(source)
  if (!status.isFile() || status.isSymbolicLink()) {
    throw new Error(`release input changed while assembling: ${name}`)
  }
  await copyFile(source, join(outputDirectory, name))
}

const assetRecords = []
for (const name of inputFiles) {
  const data = await readFile(join(outputDirectory, name))
  const buildRecord = buildRecords.find(
    (record) => name === record.archive.name || name === `yanshu-v${version}-${record.target}.build.json`
  )
  assetRecords.push({
    name,
    kind: name.endsWith('.zip') ? 'archive' : name.endsWith('.cdx.json') ? 'sbom' : 'build-record',
    target: buildRecord?.target ?? null,
    sha256: sha256(data),
    size: data.length
  })
}

const manifestName = `yanshu-v${version}.release.json`
const manifest = {
  schemaVersion: RELEASE_SCHEMA_VERSION,
  product: 'Yanshu',
  version,
  tag: `v${version}`,
  source: {
    repository: 'https://github.com/Yukk1o/yanshu',
    commit: sourceCommit,
    sourceDateEpoch
  },
  reproducibility: {
    binaryBuildsPerTarget: 2,
    deterministicArchive: 'zip-store-v1',
    sourcePathRemapped: true
  },
  sboms: sbomRecords,
  assets: assetRecords,
  verification: {
    checksums: 'SHA256SUMS',
    provenanceCommand: `gh attestation verify <asset> --repo Yukk1o/yanshu`
  }
}
await writeFile(join(outputDirectory, manifestName), canonicalJson(manifest), {
  encoding: 'utf8',
  flag: 'wx'
})

const checksumEntries = []
for (const name of [...inputFiles, manifestName]) {
  checksumEntries.push([name, sha256(await readFile(join(outputDirectory, name)))])
}
await writeFile(join(outputDirectory, 'SHA256SUMS'), formatChecksums(checksumEntries), {
  encoding: 'utf8',
  flag: 'wx'
})

process.stdout.write(
  canonicalJson({
    ok: true,
    version,
    sourceCommit,
    assets: checksumEntries.length + 1,
    outputDirectory
  })
)
