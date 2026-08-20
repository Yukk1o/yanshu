import { lstat, readFile, readdir } from 'node:fs/promises'
import { join, resolve } from 'node:path'

import { parseChecksums, requireSha256, sha256 } from './release-lib.mjs'

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
const manifestAssets = new Map(manifest.assets?.map((asset) => [asset.name, asset]) ?? [])
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
