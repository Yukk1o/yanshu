'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');

const {
  shouldExcludeTestEnvironmentName,
} = require('../scripts/test-e2e.cjs');

test('Extension Host sanitization preserves the isolated X display session', () => {
  assert.equal(shouldExcludeTestEnvironmentName('DISPLAY'), false);
  assert.equal(shouldExcludeTestEnvironmentName('XAUTHORITY'), false);
  assert.equal(shouldExcludeTestEnvironmentName('xauthority'), false);
});

test('Extension Host sanitization still removes credentials and proxies', () => {
  assert.equal(shouldExcludeTestEnvironmentName('OPENAI_API_KEY'), true);
  assert.equal(shouldExcludeTestEnvironmentName('ACCESS_TOKEN'), true);
  assert.equal(shouldExcludeTestEnvironmentName('DATABASE_PASSWORD'), true);
  assert.equal(shouldExcludeTestEnvironmentName('HTTPS_PROXY'), true);
  assert.equal(shouldExcludeTestEnvironmentName('npm_config_proxy'), true);
  assert.equal(shouldExcludeTestEnvironmentName('PATH'), false);
});
