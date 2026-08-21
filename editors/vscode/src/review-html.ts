import type { ReviewDocumentResponse } from './review-protocol';

const contentSecurityPolicy = "default-src 'none'; style-src 'unsafe-inline';";

export function renderReviewHtml(
  review: ReviewDocumentResponse,
  sourceLabel: string,
): string {
  return renderShell(
    sourceLabel,
    `<pre aria-label="Rust 风格只读审查代码"><code>${escapeHtml(review.text)}</code></pre>`,
  );
}

export function renderReviewLoadingHtml(sourceLabel: string): string {
  return renderShell(
    sourceLabel,
    '<div class="status" role="status">正在生成语义投影…</div>',
  );
}

export function renderReviewErrorHtml(sourceLabel: string): string {
  return renderShell(
    sourceLabel,
    '<div class="status error" role="alert">无法生成当前快照。请检查源文件诊断后重试。</div>',
  );
}

function renderShell(sourceLabel: string, content: string): string {
  const safeLabel = escapeHtml(sourceLabel);
  return `<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta http-equiv="Content-Security-Policy" content="${contentSecurityPolicy}">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>${safeLabel} · 只读审查</title>
  <style>
    :root {
      color-scheme: light dark;
      font-family: var(--vscode-font-family, sans-serif);
      font-size: var(--vscode-font-size, 13px);
    }

    * {
      box-sizing: border-box;
    }

    html,
    body {
      width: 100%;
      min-height: 100%;
      margin: 0;
    }

    body {
      overflow-x: hidden;
      color: var(--vscode-editor-foreground, #cccccc);
      background: var(--vscode-editor-background, #1e1e1e);
      font-size: 1rem;
    }

    main {
      width: 100%;
      padding: 0.75rem;
    }

    pre,
    .status {
      width: 100%;
      margin: 0;
      border: 1px solid var(--vscode-editorWidget-border, #454545);
      border-radius: 2px;
      background: var(--vscode-textCodeBlock-background, rgba(127, 127, 127, 0.1));
    }

    pre {
      min-height: 10rem;
      padding: 0.8rem;
      overflow: auto;
      font-family: var(--vscode-editor-font-family, monospace);
      font-size: var(--vscode-editor-font-size, 13px);
      line-height: 1.5;
      tab-size: var(--vscode-editor-tab-size, 4);
    }

    code {
      font: inherit;
    }

    .status {
      padding: 0.8rem;
      color: var(--vscode-descriptionForeground, #9d9d9d);
    }

    .status.error {
      color: var(--vscode-errorForeground, #f48771);
    }

    @media (min-width: 48rem) {
      main {
        padding: 1rem;
      }

    }
  </style>
</head>
<body>
  <main>
    ${content}
  </main>
</body>
</html>`;
}

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/gu, (character) => {
    switch (character) {
      case '&':
        return '&amp;';
      case '<':
        return '&lt;';
      case '>':
        return '&gt;';
      case '"':
        return '&quot;';
      default:
        return '&#39;';
    }
  });
}
