import { createHash } from 'node:crypto'
import { readFile, readdir } from 'node:fs/promises'
import { join } from 'node:path'

export const RELEASE_SCHEMA_VERSION = 2

export const RELEASE_PROGRAMS = Object.freeze([
  Object.freeze({
    key: 'cli',
    packageName: 'yanshu-cli',
    binaryStem: 'yanshu',
    smokeTest: 'cli-usage-json-v1'
  }),
  Object.freeze({
    key: 'mcp',
    packageName: 'yanshu-mcp',
    binaryStem: 'yanshu-mcp',
    smokeTest: 'mcp-tools-list-jsonrpc-v1'
  })
])

export const RELEASE_TARGETS = Object.freeze({
  'x86_64-pc-windows-msvc': Object.freeze({
    executableSuffix: '.exe',
    label: 'windows-x86_64'
  }),
  'x86_64-unknown-linux-gnu': Object.freeze({
    executableSuffix: '',
    label: 'linux-x86_64'
  })
})

export function releaseBinaryName(program, targetConfiguration) {
  if (!RELEASE_PROGRAMS.includes(program)) {
    throw new Error('release binary must be one of the declared programs')
  }
  if (!Object.values(RELEASE_TARGETS).includes(targetConfiguration)) {
    throw new Error('release binary target configuration is not declared')
  }
  return `${program.binaryStem}${targetConfiguration.executableSuffix}`
}

export function canonicalJson(value) {
  return `${JSON.stringify(value, null, 2)}\n`
}

export function sha256(data) {
  return createHash('sha256').update(data).digest('hex')
}

export function requireSha256(value, label = 'SHA-256') {
  if (!/^[0-9a-f]{64}$/.test(value)) {
    throw new Error(`${label} must be 64 lowercase hexadecimal characters`)
  }
  return value
}

export function requireGitCommit(value) {
  if (!/^(?:[0-9a-f]{40}|[0-9a-f]{64})$/.test(value)) {
    throw new Error('source commit must be a lowercase hexadecimal Git object ID')
  }
  return value
}

export function requireSourceEpoch(value) {
  if (!/^[1-9][0-9]*$/.test(String(value))) {
    throw new Error('source date epoch must be a positive integer')
  }
  const epoch = Number(value)
  if (!Number.isSafeInteger(epoch)) {
    throw new Error('source date epoch exceeds the JavaScript safe integer range')
  }
  return epoch
}

export function requireStableVersion(value) {
  if (!/^[0-9]+\.[0-9]+\.[0-9]+$/.test(value)) {
    throw new Error(`release version must be stable SemVer without a prefix: ${value}`)
  }
  return value
}

export function releaseRustToolchain(rustVersion) {
  if (/^[0-9]+\.[0-9]+$/.test(rustVersion)) return `${rustVersion}.0`
  if (/^[0-9]+\.[0-9]+\.[0-9]+$/.test(rustVersion)) return rustVersion
  throw new Error(`workspace rust-version is not a release toolchain version: ${rustVersion}`)
}

export function validateReleaseTag(tag, version) {
  requireStableVersion(version)
  const expected = `v${version}`
  if (tag !== expected) {
    throw new Error(`release tag ${tag} does not match workspace version ${expected}`)
  }
  return tag
}

function workspacePackageBlock(document) {
  const match = document.match(
    /(?:^|\n)\[workspace\.package\]\s*\n([\s\S]*?)(?=\n\[[^\]]+\]|$)/
  )
  if (!match) {
    throw new Error('Cargo.toml does not contain [workspace.package]')
  }
  return match[1]
}

function tomlString(block, key) {
  const expression = new RegExp(`^${key.replace('-', '\\-')}\\s*=\\s*"([^"]+)"\\s*$`, 'm')
  const match = block.match(expression)
  if (!match) {
    throw new Error(`[workspace.package] does not define ${key} as a string`)
  }
  return match[1]
}

export async function readWorkspaceReleaseMetadata(repositoryRoot) {
  const cargoToml = await readFile(join(repositoryRoot, 'Cargo.toml'), 'utf8')
  const packageBlock = workspacePackageBlock(cargoToml)
  const version = requireStableVersion(tomlString(packageBlock, 'version'))
  const rustVersion = tomlString(packageBlock, 'rust-version')

  const cratesRoot = join(repositoryRoot, 'rust', 'crates')
  const crateNames = (await readdir(cratesRoot, { withFileTypes: true }))
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort()

  for (const crateName of crateNames) {
    const manifestPath = join(cratesRoot, crateName, 'Cargo.toml')
    const manifest = await readFile(manifestPath, 'utf8')
    const packageName = manifest.match(/^name\s*=\s*"([^"]+)"\s*$/m)?.[1]
    if (packageName !== crateName) {
      throw new Error(`${crateName}/Cargo.toml package name must match its directory`)
    }
    for (const inherited of ['version', 'edition', 'rust-version', 'license', 'publish']) {
      const expression = new RegExp(`^${inherited.replace('-', '\\-')}\\.workspace\\s*=\\s*true\\s*$`, 'm')
      if (!expression.test(manifest)) {
        throw new Error(`${crateName}/Cargo.toml must inherit ${inherited} from the workspace`)
      }
    }

    const firstPartyDependencies = manifest.matchAll(
      /^(yanshu-[a-z0-9-]+)\s*=\s*\{([^\n}]*)\}\s*$/gm
    )
    for (const dependency of firstPartyDependencies) {
      const fields = dependency[2]
      const dependencyVersion = fields.match(/(?:^|,)\s*version\s*=\s*"=([^"]+)"/)?.[1]
      const dependencyPath = fields.match(/(?:^|,)\s*path\s*=\s*"([^"]+)"/)?.[1]
      if (dependencyVersion !== version || !dependencyPath) {
        throw new Error(
          `${crateName}/Cargo.toml dependency ${dependency[1]} must use a path and exact version ${version}`
        )
      }
    }
  }

  return Object.freeze({ crateCount: crateNames.length, rustVersion, version })
}

function crc32Table() {
  const table = new Uint32Array(256)
  for (let index = 0; index < table.length; index += 1) {
    let value = index
    for (let bit = 0; bit < 8; bit += 1) {
      value = (value >>> 1) ^ (value & 1 ? 0xedb88320 : 0)
    }
    table[index] = value >>> 0
  }
  return table
}

const CRC32_TABLE = crc32Table()

function crc32(data) {
  let value = 0xffffffff
  for (const byte of data) {
    value = (value >>> 8) ^ CRC32_TABLE[(value ^ byte) & 0xff]
  }
  return (value ^ 0xffffffff) >>> 0
}

function validateArchivePath(path) {
  if (
    typeof path !== 'string' ||
    path.length === 0 ||
    path.startsWith('/') ||
    path.includes('\\') ||
    path.split('/').some((segment) => segment === '' || segment === '.' || segment === '..')
  ) {
    throw new Error(`invalid release archive path: ${path}`)
  }
}

export function createDeterministicZip(entries) {
  if (!Array.isArray(entries) || entries.length === 0 || entries.length > 0xffff) {
    throw new Error('release archive must contain between 1 and 65535 files')
  }

  const sorted = entries
    .map((entry) => ({
      data: Buffer.isBuffer(entry.data) ? entry.data : Buffer.from(entry.data),
      mode: entry.mode ?? 0o100644,
      path: entry.path
    }))
    .sort((left, right) => left.path.localeCompare(right.path, 'en'))

  const names = new Set()
  const localRecords = []
  const centralRecords = []
  let localOffset = 0

  for (const entry of sorted) {
    validateArchivePath(entry.path)
    if (names.has(entry.path)) {
      throw new Error(`duplicate release archive path: ${entry.path}`)
    }
    names.add(entry.path)
    if (!Number.isInteger(entry.mode) || entry.mode < 0 || entry.mode > 0xffff) {
      throw new Error(`invalid Unix mode for ${entry.path}`)
    }

    const name = Buffer.from(entry.path, 'utf8')
    if (name.length > 0xffff || entry.data.length > 0xffffffff) {
      throw new Error(`release archive entry exceeds ZIP32 limits: ${entry.path}`)
    }
    const checksum = crc32(entry.data)
    const localHeader = Buffer.alloc(30)
    localHeader.writeUInt32LE(0x04034b50, 0)
    localHeader.writeUInt16LE(20, 4)
    localHeader.writeUInt16LE(0x0800, 6)
    localHeader.writeUInt16LE(0, 8)
    localHeader.writeUInt16LE(0, 10)
    localHeader.writeUInt16LE(0x0021, 12)
    localHeader.writeUInt32LE(checksum, 14)
    localHeader.writeUInt32LE(entry.data.length, 18)
    localHeader.writeUInt32LE(entry.data.length, 22)
    localHeader.writeUInt16LE(name.length, 26)
    localHeader.writeUInt16LE(0, 28)
    localRecords.push(localHeader, name, entry.data)

    const centralHeader = Buffer.alloc(46)
    centralHeader.writeUInt32LE(0x02014b50, 0)
    centralHeader.writeUInt16LE(0x0314, 4)
    centralHeader.writeUInt16LE(20, 6)
    centralHeader.writeUInt16LE(0x0800, 8)
    centralHeader.writeUInt16LE(0, 10)
    centralHeader.writeUInt16LE(0, 12)
    centralHeader.writeUInt16LE(0x0021, 14)
    centralHeader.writeUInt32LE(checksum, 16)
    centralHeader.writeUInt32LE(entry.data.length, 20)
    centralHeader.writeUInt32LE(entry.data.length, 24)
    centralHeader.writeUInt16LE(name.length, 28)
    centralHeader.writeUInt16LE(0, 30)
    centralHeader.writeUInt16LE(0, 32)
    centralHeader.writeUInt16LE(0, 34)
    centralHeader.writeUInt16LE(0, 36)
    centralHeader.writeUInt32LE((entry.mode << 16) >>> 0, 38)
    centralHeader.writeUInt32LE(localOffset, 42)
    centralRecords.push(centralHeader, name)

    localOffset += localHeader.length + name.length + entry.data.length
    if (localOffset > 0xffffffff) {
      throw new Error('release archive exceeds the ZIP32 size limit')
    }
  }

  const centralDirectory = Buffer.concat(centralRecords)
  if (centralDirectory.length > 0xffffffff) {
    throw new Error('release central directory exceeds the ZIP32 size limit')
  }
  const end = Buffer.alloc(22)
  end.writeUInt32LE(0x06054b50, 0)
  end.writeUInt16LE(0, 4)
  end.writeUInt16LE(0, 6)
  end.writeUInt16LE(sorted.length, 8)
  end.writeUInt16LE(sorted.length, 10)
  end.writeUInt32LE(centralDirectory.length, 12)
  end.writeUInt32LE(localOffset, 16)
  end.writeUInt16LE(0, 20)

  return Buffer.concat([...localRecords, centralDirectory, end])
}

export function parseChecksums(document) {
  const checksums = new Map()
  const lines = document.split('\n')
  if (lines.at(-1) !== '') {
    throw new Error('SHA256SUMS must end with LF')
  }
  for (const line of lines.slice(0, -1)) {
    const match = line.match(/^([0-9a-f]{64})  ([A-Za-z0-9][A-Za-z0-9._-]*)$/)
    if (!match) {
      throw new Error(`invalid SHA256SUMS line: ${line}`)
    }
    if (checksums.has(match[2])) {
      throw new Error(`duplicate SHA256SUMS entry: ${match[2]}`)
    }
    checksums.set(match[2], match[1])
  }
  if (checksums.size === 0) {
    throw new Error('SHA256SUMS cannot be empty')
  }
  return checksums
}

export function formatChecksums(entries) {
  const sorted = [...entries].sort(([left], [right]) => left.localeCompare(right, 'en'))
  return `${sorted.map(([name, digest]) => `${requireSha256(digest)}  ${name}`).join('\n')}\n`
}
