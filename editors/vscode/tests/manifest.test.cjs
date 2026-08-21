'use strict';

const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const extensionRoot = path.resolve(__dirname, '..');
const manifest = JSON.parse(readFileSync(path.join(extensionRoot, 'package.json'), 'utf8'));

test('manifest activates only for the declared Yanshu language', () => {
  assert.equal(manifest.engines.vscode, '^1.101.0');
  assert.deepEqual(manifest.activationEvents, ['onLanguage:yanshu']);
  assert.deepEqual(manifest.contributes.languages[0].extensions, ['.yan']);
  assert.equal(manifest.contributes.grammars[0].scopeName, 'source.yanshu');
  assert.equal(manifest.contributes.configuration.properties['yanshu.server.path'].scope, 'machine');
});

test('language configuration and grammar are valid JSON', () => {
  const configuration = JSON.parse(
    readFileSync(path.join(extensionRoot, 'language-configuration.json'), 'utf8'),
  );
  const grammar = JSON.parse(
    readFileSync(path.join(extensionRoot, 'syntaxes', 'yanshu.tmLanguage.json'), 'utf8'),
  );
  assert.equal(configuration.comments.lineComment, ';');
  assert.equal(grammar.scopeName, 'source.yanshu');
  assert.ok(grammar.patterns.length >= 10);
});

test('runtime and build dependencies are exactly pinned', () => {
  for (const dependencies of [manifest.dependencies, manifest.devDependencies]) {
    for (const version of Object.values(dependencies)) {
      assert.match(version, /^\d+\.\d+\.\d+$/u);
    }
  }
});
