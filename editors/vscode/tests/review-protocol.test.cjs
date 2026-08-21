'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');

const {
  maximumReviewTextBytes,
  parseReviewDocumentResponse,
  reviewLanguageId,
  reviewRenderer,
  reviewRequestMethod,
} = require('../out/review-protocol.js');

function validResponse() {
  return {
    sourceVersion: 7,
    renderer: reviewRenderer,
    editable: false,
    languageId: reviewLanguageId,
    text: '// Generated semantic review — READ ONLY.\nfn use() {}\n',
  };
}

test('review protocol accepts only the versioned read-only renderer contract', () => {
  assert.equal(reviewRequestMethod, 'yanshu/reviewDocument');
  const response = validResponse();
  assert.equal(parseReviewDocumentResponse(response, 7), response);

  assert.throws(
    () => parseReviewDocumentResponse({ ...response, sourceVersion: 6 }, 7),
    /current document version/u,
  );
  assert.throws(
    () => parseReviewDocumentResponse({ ...response, editable: true }, 7),
    /read-only renderer contract/u,
  );
  assert.throws(
    () => parseReviewDocumentResponse({ ...response, renderer: 'rust-source-v1' }, 7),
    /read-only renderer contract/u,
  );
  assert.throws(
    () => parseReviewDocumentResponse({ ...response, languageId: 'yanshu' }, 7),
    /unsupported display language/u,
  );
});

test('review protocol rejects missing markers and oversized text', () => {
  const response = validResponse();
  assert.throws(
    () => parseReviewDocumentResponse({ ...response, text: 'fn use() {}' }, 7),
    /marker or exceeds/u,
  );
  assert.throws(
    () => parseReviewDocumentResponse({
      ...response,
      text: `${response.text}${'x'.repeat(maximumReviewTextBytes)}`,
    }, 7),
    /marker or exceeds/u,
  );
});
