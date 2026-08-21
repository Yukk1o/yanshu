import { Buffer } from 'node:buffer';

export const reviewRequestMethod = 'yanshu/reviewDocument';
export const reviewRenderer = 'rust-readonly-v3';
export const reviewLanguageId = 'rust';
export const maximumReviewTextBytes = 4 * 1024 * 1024;

const reviewHeader = '// Generated semantic review — READ ONLY.';

export interface ReviewDocumentResponse {
  readonly sourceVersion: number;
  readonly renderer: typeof reviewRenderer;
  readonly editable: false;
  readonly languageId: typeof reviewLanguageId;
  readonly text: string;
}

export function parseReviewDocumentResponse(
  value: unknown,
  expectedVersion: number,
): ReviewDocumentResponse {
  if (!isRecord(value)) {
    throw new Error('review response must be an object');
  }
  if (!Number.isSafeInteger(value.sourceVersion) || value.sourceVersion !== expectedVersion) {
    throw new Error('review response does not match the current document version');
  }
  if (value.renderer !== reviewRenderer || value.editable !== false) {
    throw new Error('review response violated the read-only renderer contract');
  }
  if (value.languageId !== reviewLanguageId) {
    throw new Error('review response uses an unsupported display language');
  }
  if (
    typeof value.text !== 'string'
    || !value.text.startsWith(reviewHeader)
    || Buffer.byteLength(value.text, 'utf8') > maximumReviewTextBytes
  ) {
    throw new Error('review response text is missing its marker or exceeds the byte limit');
  }
  return value as unknown as ReviewDocumentResponse;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
