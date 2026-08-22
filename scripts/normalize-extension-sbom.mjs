import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import {
  RELEASE_EXTENSION,
  canonicalJson,
  releaseSbomRootReference,
  requireGitCommit,
  requireSourceEpoch,
  requireStableVersion
} from './release-lib.mjs'

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

function requiredOption(name) {
  const index = process.argv.indexOf(name)
  const value = index === -1 ? undefined : process.argv[index + 1]
  if (!value || value.startsWith('--')) throw new Error(`${name} requires a value`)
  return value
}

const input = resolve(repositoryRoot, requiredOption('--input'))
const output = resolve(repositoryRoot, requiredOption('--output'))
const version = requireStableVersion(requiredOption('--version'))
const sourceCommit = requireGitCommit(requiredOption('--source-commit'))
const sourceDateEpoch = requireSourceEpoch(requiredOption('--source-date-epoch'))
const sbom = JSON.parse(await readFile(input, 'utf8'))
const root = sbom.metadata?.component
const expectedPurl = `pkg:npm/${RELEASE_EXTENSION.packageName}@${version}`

if (sbom.bomFormat !== 'CycloneDX' || sbom.specVersion !== '1.5') {
  throw new Error('extension SBOM must be CycloneDX 1.5 JSON')
}
if (
  !root ||
  root.version !== version ||
  root.purl !== expectedPurl ||
  typeof root['bom-ref'] !== 'string'
) {
  throw new Error('extension SBOM root does not match the locked extension package')
}

const originalRootReference = root['bom-ref']
const canonicalRootReference = releaseSbomRootReference(
  RELEASE_EXTENSION,
  version,
  sourceCommit
)
root['bom-ref'] = canonicalRootReference
root.name = RELEASE_EXTENSION.packageName

function rewriteReferences(value) {
  if (typeof value === 'string') {
    return value === originalRootReference ? canonicalRootReference : value
  }
  if (Array.isArray(value)) return value.map(rewriteReferences)
  if (value && typeof value === 'object') {
    for (const [key, child] of Object.entries(value)) value[key] = rewriteReferences(child)
  }
  return value
}
rewriteReferences(sbom)

delete sbom.serialNumber
sbom.metadata.timestamp = new Date(sourceDateEpoch * 1000).toISOString()
const yanshuProperties = [
  { name: 'yanshu:bundled-program', value: 'lsp' },
  { name: 'yanshu:release-program', value: RELEASE_EXTENSION.key },
  { name: 'yanshu:source-commit', value: sourceCommit },
  { name: 'yanshu:source-date-epoch', value: String(sourceDateEpoch) }
]
const existingProperties = (sbom.metadata.properties ?? []).filter(
  (property) => !yanshuProperties.some((candidate) => candidate.name === property.name)
)
sbom.metadata.properties = [...existingProperties, ...yanshuProperties].sort((left, right) =>
  left.name.localeCompare(right.name, 'en')
)
if (Array.isArray(sbom.components)) {
  sbom.components.sort((left, right) => left['bom-ref'].localeCompare(right['bom-ref'], 'en'))
}
if (Array.isArray(sbom.dependencies)) {
  for (const dependency of sbom.dependencies) dependency.dependsOn?.sort()
  sbom.dependencies.sort((left, right) => left.ref.localeCompare(right.ref, 'en'))
}

const normalizedDocument = canonicalJson(sbom)
if (
  normalizedDocument.includes('path+file:') ||
  normalizedDocument.includes('download_url=file:') ||
  /(?:^|["\s])[A-Za-z]:[\\/]/m.test(normalizedDocument) ||
  normalizedDocument.includes('/home/runner/')
) {
  throw new Error('normalized extension SBOM still contains a checkout-local path')
}

await mkdir(dirname(output), { recursive: true })
await writeFile(output, normalizedDocument, { encoding: 'utf8', flag: 'wx' })
process.stdout.write(
  canonicalJson({
    components: Array.isArray(sbom.components) ? sbom.components.length : 0,
    ok: true,
    output,
    program: RELEASE_EXTENSION.key,
    sourceCommit,
    version
  })
)
