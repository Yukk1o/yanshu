'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');

const {
  renderReviewErrorHtml,
  renderReviewHtml,
  renderReviewLoadingHtml,
} = require('../out/review-html.js');

const review = {
  sourceVersion: 9,
  renderer: 'rust-readonly-v3',
  editable: false,
  languageId: 'rust',
  text: '// Generated semantic review — READ ONLY.\n<script>alert("never")</script>\n',
};

test('review panel is scriptless, escaped, and visibly read only', () => {
  const html = renderReviewHtml(review, '<img src=x onerror=alert(1)>');
  assert.match(
    html,
    /Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline';"/u,
  );
  assert.doesNotMatch(html, /<script/iu);
  assert.doesNotMatch(html, /<img src=x/iu);
  assert.match(html, /&lt;script&gt;alert\(&quot;never&quot;\)&lt;\/script&gt;/u);
  assert.match(html, /&lt;img src=x onerror=alert\(1\)&gt;/u);
  assert.match(html, /READ ONLY/u);
  assert.match(html, /不可执行/u);
  assert.doesNotMatch(html, /contenteditable/iu);
});

test('review panel uses a fluid mobile-first audit layout', () => {
  const html = renderReviewHtml(review, 'policy.yan');
  assert.match(html, /width: 100%/u);
  assert.match(html, /overflow: auto/u);
  assert.match(html, /@media \(min-width: 48rem\)/u);
  assert.match(html, /font-size: 1rem/u);
  assert.match(html, /snapshot 9/u);
  assert.match(renderReviewLoadingHtml('policy.yan', 9), /正在生成语义投影/u);
  assert.match(renderReviewErrorHtml('policy.yan', 9), /无法生成当前快照/u);
});
