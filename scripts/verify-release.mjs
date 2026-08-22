import { lstat, readFile, readdir } from 'node:fs/promises'
import { join, resolve } from 'node:path'

import {
  RELEASE_EXTENSION,
  RELEASE_PROGRAMS,
  RELEASE_SCHEMA_VERSION,
  RELEASE_SBOM_COMPONENTS,
  RELEASE_TARGETS,
  parseChecksums,
  requireGitCommit,
  releaseSbomRootReference,
  releaseVsixName,
  requireSha256,
  requireSourceEpoch,
  requireStableVersion,
  sha256
} from './release-lib.mjs'

const directoryArgument = process.argv[2]
if (!directoryArgument || directoryArgument.startsWith('--')) {
  throw new Error('usage: node scripts/verify-release.mjs <release-directory>')
}
const releaseDirectory = resolve(directoryArgument)
const files = (await readdir(releaseDirectory, { withFileTypes: true }))
  .map((entry) => {
    if (!entry.isFile() || entry.isSymbolicLink()) {
      throw new Error(`release directory must contain only regular files: ${entry.name}`)
    }
    return entry.name
  })
  .sort((left, right) => left.localeCompare(right, 'en'))
const checksums = parseChecksums(await readFile(join(releaseDirectory, 'SHA256SUMS'), 'utf8'))
const expectedFiles = files.filter((name) => name !== 'SHA256SUMS')
if (
  checksums.size !== expectedFiles.length ||
  expectedFiles.some((name) => !checksums.has(name))
) {
  throw new Error('SHA256SUMS does not cover every release file exactly once')
}

for (const [name, expected] of checksums) {
  const status = await lstat(join(releaseDirectory, name))
  if (!status.isFile() || status.isSymbolicLink()) {
    throw new Error(`checksum target is not a regular file: ${name}`)
  }
  const actual = sha256(await readFile(join(releaseDirectory, name)))
  if (actual !== expected) {
    throw new Error(`checksum mismatch for ${name}: ${actual} != ${expected}`)
  }
}

const manifestNames = files.filter((name) => name.endsWith('.release.json'))
if (manifestNames.length !== 1) throw new Error('release must contain exactly one release manifest')
const manifest = JSON.parse(await readFile(join(releaseDirectory, manifestNames[0]), 'utf8'))
const version = requireStableVersion(manifest.version)
if (
  manifestNames[0] !== `yanshu-v${version}.release.json` ||
  manifest.schemaVersion !== RELEASE_SCHEMA_VERSION ||
  manifest.product !== 'Yanshu' ||
  manifest.tag !== `v${version}` ||
  manifest.source?.repository !== 'https://github.com/Yukk1o/yanshu'
) {
  throw new Error('release manifest identity is invalid')
}
const sourceCommit = requireGitCommit(manifest.source?.commit)
requireSourceEpoch(manifest.source?.sourceDateEpoch)
const expectedSboms = new Map(
  RELEASE_SBOM_COMPONENTS.map((program) => [
    program.key,
    {
      file: `yanshu-v${version}-${program.key}.cdx.json`,
      package: program.packageName
    }
  ])
)
const manifestSbomPrograms = new Set(manifest.sboms?.map((sbom) => sbom.program) ?? [])
if (
  !Array.isArray(manifest.sboms) ||
  manifest.sboms.length !== expectedSboms.size ||
  manifestSbomPrograms.size !== expectedSboms.size ||
  manifest.sboms.some(
    (sbom) => {
      const expected = expectedSboms.get(sbom.program)
      return (
        sbom.file !== expected?.file ||
        sbom.package !== expected?.package ||
        sbom.format !== 'CycloneDX' ||
        sbom.specVersion !== '1.5'
      )
    }
  )
) {
  throw new Error('release manifest SBOM set is invalid')
}
for (const sbomRecord of manifest.sboms) {
  const sbom = JSON.parse(await readFile(join(releaseDirectory, sbomRecord.file), 'utf8'))
  const properties = sbom.metadata?.properties ?? []
  const sourceCommitProperties = properties.filter(
    (property) => property.name === 'yanshu:source-commit'
  )
  const releaseProgramProperties = properties.filter(
    (property) => property.name === 'yanshu:release-program'
  )
  const bundledProgramProperties = properties.filter(
    (property) => property.name === 'yanshu:bundled-program'
  )
  const component = RELEASE_SBOM_COMPONENTS.find(
    (candidate) => candidate.key === sbomRecord.program
  )
  const expectedRootReference = releaseSbomRootReference(component, version, sourceCommit)
  if (
    sbom.bomFormat !== 'CycloneDX' ||
    sbom.specVersion !== '1.5' ||
    sbom.metadata?.component?.version !== version ||
    sbom.metadata.component['bom-ref'] !== expectedRootReference ||
    sourceCommitProperties.length !== 1 ||
    sourceCommitProperties[0].value !== sourceCommit ||
    releaseProgramProperties.length !== 1 ||
    releaseProgramProperties[0].value !== sbomRecord.program ||
    (component === RELEASE_EXTENSION &&
      (bundledProgramProperties.length !== 1 || bundledProgramProperties[0].value !== 'lsp')) ||
    (component !== RELEASE_EXTENSION && bundledProgramProperties.length !== 0)
  ) {
    throw new Error(`release SBOM does not describe ${sbomRecord.program}`)
  }
}
const manifestAssets = new Map(manifest.assets?.map((asset) => [asset.name, asset]) ?? [])
const expectedAssets = new Map()
for (const [target, configuration] of Object.entries(RELEASE_TARGETS)) {
  const stem = `yanshu-v${version}-${target}`
  expectedAssets.set(`${stem}.zip`, { kind: 'archive', target })
  expectedAssets.set(`${stem}.build.json`, { kind: 'build-record', target })
  expectedAssets.set(releaseVsixName(version, configuration), { kind: 'extension', target })
}
for (const sbom of expectedSboms.values()) {
  expectedAssets.set(sbom.file, { kind: 'sbom', target: null })
}
if (
  manifestAssets.size !== expectedAssets.size ||
  [...expectedAssets].some(([name, expected]) => {
    const actual = manifestAssets.get(name)
    return actual?.kind !== expected.kind || actual.target !== expected.target
  })
) {
  throw new Error('release manifest asset set is invalid')
}
for (const asset of manifestAssets.values()) {
  requireSha256(asset.sha256, `${asset.name} manifest digest`)
  if (checksums.get(asset.name) !== asset.sha256) {
    throw new Error(`release manifest digest does not match SHA256SUMS for ${asset.name}`)
  }
  const status = await lstat(join(releaseDirectory, asset.name))
  if (status.size !== asset.size) {
    throw new Error(`release manifest size does not match ${asset.name}`)
  }
}
const nonManifestAssets = expectedFiles.filter((name) => name !== manifestNames[0])
if (
  manifestAssets.size !== nonManifestAssets.length ||
  nonManifestAssets.some((name) => !manifestAssets.has(name))
) {
  throw new Error('release manifest does not cover every payload asset exactly once')
}

process.stdout.write(
  `${JSON.stringify({
    ok: true,
    version: manifest.version,
    sourceCommit: manifest.source?.commit,
    verifiedFiles: checksums.size
  })}\n`
)
