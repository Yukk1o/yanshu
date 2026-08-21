'use strict';

const assert = require('node:assert/strict');
const path = require('node:path');
const test = require('node:test');

const {
  platformTarget,
  sanitizedServerEnvironment,
  selectServerCommand,
  serverExecutableName,
} = require('../out/server-command.js');

test('platform target and executable names are explicit', () => {
  assert.equal(platformTarget('win32', 'x64'), 'win32-x64');
  assert.equal(platformTarget('darwin', 'arm64'), 'darwin-arm64');
  assert.equal(platformTarget('freebsd', 'x64'), undefined);
  assert.equal(serverExecutableName('win32'), 'yanshu-lsp.exe');
  assert.equal(serverExecutableName('linux'), 'yanshu-lsp');
});

test('an absolute configured server takes precedence', () => {
  const configured = path.resolve('trusted', serverExecutableName());
  const selection = selectServerCommand({
    extensionPath: path.resolve('extension'),
    configuredPath: configured,
    platform: process.platform,
    architecture: process.arch,
    isRegularFile: (candidate) => candidate === configured,
  });
  assert.deepEqual(selection, { command: configured, source: 'configured' });
});

test('relative or missing configured paths fail closed', () => {
  assert.throws(
    () => selectServerCommand({
      extensionPath: path.resolve('extension'),
      configuredPath: `relative${path.sep}yanshu-lsp`,
      isRegularFile: () => true,
    }),
    /must be absolute/,
  );
  assert.throws(
    () => selectServerCommand({
      extensionPath: path.resolve('extension'),
      configuredPath: path.resolve('missing', 'yanshu-lsp'),
      isRegularFile: () => false,
    }),
    /regular file/,
  );
});

test('bundled server precedes the host PATH fallback', () => {
  const extensionPath = path.resolve('extension');
  const bundled = path.join(extensionPath, 'server', 'linux-x64', 'yanshu-lsp');
  const selected = selectServerCommand({
    extensionPath,
    platform: 'linux',
    architecture: 'x64',
    isRegularFile: (candidate) => candidate === bundled,
  });
  assert.deepEqual(selected, { command: bundled, source: 'bundled' });

  const fallback = selectServerCommand({
    extensionPath,
    platform: 'linux',
    architecture: 'x64',
    isRegularFile: () => false,
  });
  assert.deepEqual(fallback, { command: 'yanshu-lsp', source: 'path' });
});

test('server child environment excludes credential-shaped names', () => {
  assert.deepEqual(
    sanitizedServerEnvironment({
      PATH: '/trusted/bin',
      LANG: 'zh_CN.UTF-8',
      OPENAI_API_KEY: 'never-forward',
      ACCESS_TOKEN: 'never-forward',
      DATABASE_PASSWORD: 'never-forward',
    }),
    { PATH: '/trusted/bin', LANG: 'zh_CN.UTF-8' },
  );
});
