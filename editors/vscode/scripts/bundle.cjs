'use strict';

const { mkdir, rm } = require('node:fs/promises');
const path = require('node:path');

const { build } = require('esbuild');

const extensionRoot = path.resolve(__dirname, '..');
const outputRoot = path.join(extensionRoot, 'out');

async function main() {
  if (path.dirname(outputRoot) !== extensionRoot) {
    throw new Error('refusing to replace output outside the extension directory');
  }
  await rm(outputRoot, { recursive: true, force: true });
  await mkdir(outputRoot, { recursive: true });

  await build({
    entryPoints: [path.join(extensionRoot, 'src', 'extension.ts')],
    outfile: path.join(outputRoot, 'extension.js'),
    bundle: true,
    external: ['vscode'],
    format: 'cjs',
    platform: 'node',
    target: 'node22',
    sourcemap: 'external',
    sourcesContent: false,
    logLevel: 'warning',
  });
  await build({
    entryPoints: [path.join(extensionRoot, 'src', 'server-command.ts')],
    outfile: path.join(outputRoot, 'server-command.js'),
    bundle: true,
    format: 'cjs',
    platform: 'node',
    target: 'node22',
    sourcemap: 'external',
    sourcesContent: false,
    logLevel: 'warning',
  });
  await build({
    entryPoints: [path.join(extensionRoot, 'src', 'review-protocol.ts')],
    outfile: path.join(outputRoot, 'review-protocol.js'),
    bundle: true,
    format: 'cjs',
    platform: 'node',
    target: 'node22',
    sourcemap: 'external',
    sourcesContent: false,
    logLevel: 'warning',
  });
  await build({
    entryPoints: [path.join(extensionRoot, 'src', 'review-html.ts')],
    outfile: path.join(outputRoot, 'review-html.js'),
    bundle: true,
    format: 'cjs',
    platform: 'node',
    target: 'node22',
    sourcemap: 'external',
    sourcesContent: false,
    logLevel: 'warning',
  });
  await build({
    entryPoints: [path.join(extensionRoot, 'src', 'test', 'suite', 'index.ts')],
    outfile: path.join(outputRoot, 'test', 'suite', 'index.js'),
    bundle: true,
    external: ['vscode'],
    format: 'cjs',
    platform: 'node',
    target: 'node22',
    sourcemap: 'external',
    sourcesContent: false,
    logLevel: 'warning',
  });
}

main().catch((error) => {
  const message = error instanceof Error ? error.message : 'unknown bundle failure';
  process.stderr.write(`VSCODE_BUNDLE_FAILED: ${message}\n`);
  process.exitCode = 1;
});
