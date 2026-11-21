import { isRecord, LspPosition } from './sanitise';

const MAX_URI_LENGTH = 4096;

/** A `file:` or `untitled:` URI string from a server command, at most 4096 chars; else `undefined`. */
export function asDocumentUri(value: unknown): string | undefined {
  if (typeof value !== 'string' || value.length === 0 || value.length > MAX_URI_LENGTH) {
    return undefined;
  }
  return value.startsWith('file:') || value.startsWith('untitled:') ? value : undefined;
}

/** A `{ line, character }` of non-negative safe integers (extra keys ignored), else `undefined`. */
export function asPosition(value: unknown): LspPosition | undefined {
  if (!isRecord(value)) {
    return undefined;
  }
  const { line, character } = value;
  if (typeof line !== 'number' || typeof character !== 'number') {
    return undefined;
  }
  return Number.isSafeInteger(line) && line >= 0 && Number.isSafeInteger(character) && character >= 0 ? { line, character } : undefined;
}
