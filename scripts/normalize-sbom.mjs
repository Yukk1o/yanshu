import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import {
  RELEASE_PROGRAMS,
  canonicalJson,
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
const programKey = requiredOption('--program')
const releaseProgram = RELEASE_PROGRAMS.find((program) => program.key === programKey)
if (!releaseProgram) throw new Error(`unsupported release program: ${programKey}`)
const sourceCommit = requireGitCommit(requiredOption('--source-commit'))
const sourceDateEpoch = requireSourceEpoch(requiredOption('--source-date-epoch'))
const sbom = JSON.parse(await readFile(input, 'utf8'))

if (sbom.bomFormat !== 'CycloneDX' || sbom.specVersion !== '1.5') {
  throw new Error('SBOM must be CycloneDX 1.5 JSON')
}
if (!sbom.metadata?.component || sbom.metadata.component.version !== version) {
  throw new Error('SBOM root component version does not match the workspace version')
}

const firstPartyReferenceMap = new Map()
const components = [sbom.metadata.component, ...(sbom.components ?? [])]
for (const component of components) {
  const packageMatch = component.purl?.match(/^pkg:cargo\/(yanshu(?:-[a-z0-9-]+)?)@([^?#]+)\?download_url=file:/)
  if (!packageMatch) continue
  const [, crateName, crateVersion] = packageMatch
  if (crateVersion !== version || typeof component['bom-ref'] !== 'string') {
    throw new Error(`first-party SBOM component has inconsistent identity: ${crateName}`)
  }
  const canonicalReference = `urn:yanshu:crate:${crateName}:${version}:${sourceCommit}`
  firstPartyReferenceMap.set(component['bom-ref'], canonicalReference)
  component['bom-ref'] = canonicalReference
  const source = encodeURIComponent(`https://github.com/Yukk1o/yanshu@${sourceCommit}`)
  component.purl = `pkg:cargo/${crateName}@${version}?vcs_url=${source}#rust/crates/${crateName}`
}
if (!firstPartyReferenceMap.has(sbom.metadata.component['bom-ref']) && !sbom.metadata.component['bom-ref'].startsWith('urn:yanshu:crate:')) {
  throw new Error('SBOM root component was not recognized as first-party source')
}
const expectedRootReference =
  `urn:yanshu:crate:${releaseProgram.packageName}:${version}:${sourceCommit}`
if (sbom.metadata.component['bom-ref'] !== expectedRootReference) {
  throw new Error(`SBOM root component does not describe ${releaseProgram.packageName}`)
}

function rewriteReferences(value) {
  if (typeof value === 'string') return firstPartyReferenceMap.get(value) ?? value
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
  { name: 'yanshu:source-commit', value: sourceCommit },
  { name: 'yanshu:source-date-epoch', value: String(sourceDateEpoch) },
  { name: 'yanshu:release-program', value: releaseProgram.key }
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
  throw new Error('normalized SBOM still contains a checkout-local path')
}

await mkdir(dirname(output), { recursive: true })
await writeFile(output, normalizedDocument, { encoding: 'utf8', flag: 'wx' })
process.stdout.write(
  canonicalJson({
    ok: true,
    output,
    components: Array.isArray(sbom.components) ? sbom.components.length : 0,
    program: releaseProgram.key,
    sourceCommit,
    version
  })
)
