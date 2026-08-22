import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const MAXIMUM_FILES = 64;
const MAXIMUM_FILE_BYTES = 4 * 1024 * 1024;
const MAXIMUM_TOTAL_BYTES = 16 * 1024 * 1024;
const MAXIMUM_ENTRIES = 1024;
const MAXIMUM_OUTPUT_BYTES = 32 * 1024 * 1024;

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const grammarDirectory = path.resolve(scriptDirectory, '..');
const repositoryRoot = path.resolve(grammarDirectory, '..', '..');
const treeSitter = path.join(
  grammarDirectory,
  'node_modules',
  'tree-sitter-cli',
  process.platform === 'win32' ? 'tree-sitter.exe' : 'tree-sitter',
);

let visitedEntries = 0;

function collectYanSources(root, skipInvalid) {
  const sources = [];

  function visit(directory) {
    const entries = fs.readdirSync(directory, { withFileTypes: true });
    entries.sort((left, right) => left.name.localeCompare(right.name, 'en'));
    for (const entry of entries) {
      visitedEntries += 1;
      if (visitedEntries > MAXIMUM_ENTRIES) {
        throw new Error('source discovery exceeded its entry limit');
      }
      const candidate = path.join(directory, entry.name);
      if (entry.isSymbolicLink()) {
        throw new Error(
          'source discovery refuses symlink: ' +
            path.relative(repositoryRoot, candidate),
        );
      }
      if (entry.isDirectory()) {
        if (skipInvalid && entry.name === 'invalid') {
          continue;
        }
        visit(candidate);
      } else if (entry.isFile() && entry.name.endsWith('.yan')) {
        sources.push(candidate);
      }
    }
  }

  visit(root);
  return sources;
}

const sources = [
  ...collectYanSources(path.join(repositoryRoot, 'conformance'), true),
  ...collectYanSources(path.join(repositoryRoot, 'examples'), false),
  ...collectYanSources(path.join(grammarDirectory, 'test', 'fixtures'), false),
];
sources.sort((left, right) => left.localeCompare(right, 'en'));

if (sources.length === 0 || sources.length > MAXIMUM_FILES) {
  throw new Error(
    'valid source set must contain 1..' +
      MAXIMUM_FILES +
      ' files; observed ' +
      sources.length,
  );
}

let totalBytes = 0;
for (const source of sources) {
  const metadata = fs.lstatSync(source);
  if (!metadata.isFile() || metadata.isSymbolicLink()) {
    throw new Error(
      'valid source must be a non-symlink file: ' +
        path.relative(repositoryRoot, source),
    );
  }
  if (metadata.size > MAXIMUM_FILE_BYTES) {
    throw new Error(
      'valid source exceeds the Reader byte limit: ' +
        path.relative(repositoryRoot, source),
    );
  }
  totalBytes += metadata.size;
  if (totalBytes > MAXIMUM_TOTAL_BYTES) {
    throw new Error('valid source set exceeds its aggregate byte limit');
  }
}

function run(command, arguments_, options = {}) {
  const result = spawnSync(command, arguments_, {
    cwd: options.cwd || repositoryRoot,
    encoding: 'utf8',
    maxBuffer: MAXIMUM_OUTPUT_BYTES,
    timeout: options.timeout || 120_000,
    windowsHide: true,
  });
  if (result.error) {
    throw new Error('failed to start ' + options.label, { cause: result.error });
  }
  if (result.status !== 0) {
    const output = ((result.stderr || '') + (result.stdout || '')).slice(0, 8192);
    throw new Error(options.label + ' failed:\n' + output);
  }
  return result.stdout;
}

for (const source of sources) {
  const relative = path.relative(repositoryRoot, source);
  run(treeSitter, ['parse', '--quiet', source], {
    cwd: grammarDirectory,
    label: 'Tree-sitter parse for ' + relative,
    timeout: 10_000,
  });
}

const multipleForms = path.join(
  repositoryRoot,
  'conformance',
  'v1',
  'invalid',
  'multiple-forms.yan',
);
const invalidResult = spawnSync(
  treeSitter,
  ['parse', '--quiet', multipleForms],
  {
    cwd: grammarDirectory,
    encoding: 'utf8',
    maxBuffer: MAXIMUM_OUTPUT_BYTES,
    timeout: 10_000,
    windowsHide: true,
  },
);
if (invalidResult.error) {
  throw new Error('failed to run the multiple-form rejection check', {
    cause: invalidResult.error,
  });
}
if (invalidResult.status === 0) {
  throw new Error('Tree-sitter accepted multiple top-level forms without an error node');
}

run('cargo', ['build', '--quiet', '--locked', '-p', 'yanshu-cli'], {
  label: 'canonical yanshu-cli build',
  timeout: 300_000,
});
const metadataOutput = run(
  'cargo',
  ['metadata', '--locked', '--no-deps', '--format-version', '1'],
  { label: 'Cargo metadata' },
);
const metadata = JSON.parse(metadataOutput);
const cliPackage = metadata.packages.find(
  (candidate) => candidate.name === 'yanshu-cli',
);
const cliTarget = cliPackage?.targets.find((target) =>
  target.kind.includes('bin'),
);
if (!cliTarget) {
  throw new Error('Cargo metadata does not contain the yanshu-cli binary target');
}
const cli = path.join(
  metadata.target_directory,
  'debug',
  process.platform === 'win32' ? cliTarget.name + '.exe' : cliTarget.name,
);

for (const source of sources) {
  const relative = path.relative(repositoryRoot, source);
  const output = run(cli, ['format', source], {
    label: 'canonical Reader/Parser round trip for ' + relative,
    timeout: 10_000,
  });
  let report;
  try {
    report = JSON.parse(output);
  } catch (error) {
    throw new Error(
      'canonical Reader/Parser returned non-JSON output for ' + relative,
      { cause: error },
    );
  }
  if (report.ok !== true) {
    throw new Error('canonical Reader/Parser did not accept ' + relative);
  }
}

process.stdout.write(
  'ok - ' +
    sources.length +
    ' bounded .yan sources accepted by Tree-sitter and the canonical Parser; multiple forms rejected on ' +
    os.platform() +
    '\n',
);
