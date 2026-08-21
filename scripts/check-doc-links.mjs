import { readdir, readFile, stat } from 'node:fs/promises'
import { dirname, extname, isAbsolute, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const ignoredDirectories = new Set([
  '.agents',
  '.codex',
  '.git',
  '.runtime',
  '.toolchains',
  '.vitepress',
  'build',
  'compiled',
  'node_modules',
  'public',
  'target'
])
const markdownLink = /\]\(([^)]+)\)/g
const failures = []

for (const file of await markdownFiles(repositoryRoot)) {
  const source = await readFile(file, 'utf8')
  for (const match of source.matchAll(markdownLink)) {
    const rawTarget = match[1].trim()
    if (
      rawTarget.length === 0 ||
      rawTarget.startsWith('#') ||
      rawTarget.startsWith('/') ||
      /^[a-z][a-z0-9+.-]*:/i.test(rawTarget)
    ) {
      continue
    }
    const withoutTitle = rawTarget.split(/\s+["']/u, 1)[0]
    const withoutAnchor = withoutTitle.split('#', 1)[0]
    const target = withoutAnchor.startsWith('<') && withoutAnchor.endsWith('>')
      ? withoutAnchor.slice(1, -1)
      : withoutAnchor
    if (target.length === 0) continue
    let decodedTarget
    try {
      decodedTarget = decodeURIComponent(target)
    } catch {
      failures.push(`${displayPath(file)} -> ${rawTarget} is not valid URL encoding`)
      continue
    }
    const resolved = resolve(dirname(file), decodedTarget)
    const repositoryRelative = relative(repositoryRoot, resolved)
    if (repositoryRelative.startsWith('..') || isAbsolute(repositoryRelative)) {
      failures.push(`${displayPath(file)} -> ${rawTarget} escapes the repository`)
      continue
    }
    try {
      await stat(resolved)
    } catch {
      failures.push(`${displayPath(file)} -> ${rawTarget} does not exist`)
    }
  }
}

if (failures.length > 0) {
  for (const failure of failures) console.error(failure)
  process.exitCode = 1
} else {
  console.log('ok - repository Markdown links resolve locally')
}

async function markdownFiles(directory) {
  const files = []
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (!ignoredDirectories.has(entry.name)) {
        files.push(...await markdownFiles(join(directory, entry.name)))
      }
    } else if (entry.isFile() && extname(entry.name).toLowerCase() === '.md') {
      files.push(join(directory, entry.name))
    }
  }
  return files
}

function displayPath(path) {
  return path.slice(repositoryRoot.length + 1).replaceAll('\\', '/')
}
