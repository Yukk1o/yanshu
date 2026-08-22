import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const grammarDirectory = path.resolve(scriptDirectory, '..');
const treeSitter = path.join(
  grammarDirectory,
  'node_modules',
  'tree-sitter-cli',
  process.platform === 'win32' ? 'tree-sitter.exe' : 'tree-sitter',
);
const fixture = path.join(grammarDirectory, 'test', 'fixtures', 'all-forms.yan');
const queries = [
  'queries/highlights.scm',
  'queries/locals.scm',
  'queries/folds.scm',
  'queries/tags.scm',
];

function boundedOutput(result) {
  return ((result.stderr || '') + (result.stdout || '')).slice(0, 4096);
}

for (const query of queries) {
  const result = spawnSync(
    treeSitter,
    ['query', '--quiet', path.join(grammarDirectory, query), fixture],
    {
      cwd: grammarDirectory,
      encoding: 'utf8',
      maxBuffer: 1024 * 1024,
      timeout: 60_000,
      windowsHide: true,
    },
  );
  if (result.error) {
    throw new Error('failed to run Tree-sitter query check for ' + query, {
      cause: result.error,
    });
  }
  if (result.status !== 0) {
    throw new Error(
      'Tree-sitter rejected ' + query + ':\n' + boundedOutput(result),
    );
  }
}

process.stdout.write('ok - ' + queries.length + ' Tree-sitter queries compiled\n');
