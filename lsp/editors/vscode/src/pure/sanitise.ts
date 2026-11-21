/** Size caps applied to everything a server (or a malicious model file) can make a webview hold. */
export const LIMITS = {
  /** Longest model name kept; longer names are cut with an ellipsis. */
  name: 256,
  /** Longest free text (markdown, messages) kept. */
  text: 100_000,
  nodes: 20_000,
  edges: 200_000,
  matrixEntries: 250_000,
  /** Rows or columns in one matrix window (contract: entries limit max 5000). */
  matrixAxis: 5_000,
  /** Bins per axis in a density result (contract max 512). */
  bins: 512,
  /** Rows kept per list in a table (diff sections, solution tables). */
  listRows: 5_000,
  /** Longest entry description kept (the server cuts at 1000 characters). */
  description: 1_000,
  terms: 10_000,
  related: 100,
  components: 50,
  changesPerRow: 50,
} as const;

const HTML_ESCAPES: Readonly<Record<string, string>> = {
  '&': '&amp;',
  '<': '&lt;',
  '>': '&gt;',
  '"': '&quot;',
  "'": '&#39;',
  '`': '&#96;',
};

/** Escapes text for use in HTML element content or a quoted attribute value. */
export function escapeHtml(text: string): string {
  return text.replace(/[&<>"'`]/g, (ch) => HTML_ESCAPES[ch]);
}

// C0/C1 controls (except tab), DEL, bidirectional overrides/isolates, zero-width and line/paragraph separators.
// Bidi characters let a name render as a different one ("Trojan Source").
const UNSAFE_RANGES: readonly (readonly [number, number])[] = [
  [0x00, 0x08],
  [0x0a, 0x1f],
  [0x7f, 0x9f],
  [0xad, 0xad],
  [0x61c, 0x61c],
  [0x200b, 0x200f],
  [0x2028, 0x202e],
  [0x2060, 0x206f],
  [0xfeff, 0xfeff],
];
const hex = (code: number): string => '\\u' + code.toString(16).padStart(4, '0');
const UNSAFE_CHARS = new RegExp(`[${UNSAFE_RANGES.map(([lo, hi]) => hex(lo) + '-' + hex(hi)).join('')}]`, 'g');

/** Strips control and bidirectional characters, then cuts to `max` code units with an ellipsis. */
export function cleanText(value: unknown, max: number): string {
  if (typeof value !== 'string') {
    return '';
  }
  const stripped = value.replace(UNSAFE_CHARS, '');
  return stripped.length > max ? `${stripped.slice(0, Math.max(0, max - 1))}…` : stripped;
}

/** A model name safe to display. Non-strings become the empty string. */
export function cleanName(value: unknown): string {
  return cleanText(value, LIMITS.name);
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** A finite number, else `undefined` (JSON has no NaN/Infinity, but a buggy server might send a string). */
export function asNumber(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

/** A non-negative integer, else `undefined`. */
export function asCount(value: unknown): number | undefined {
  const n = asNumber(value);
  return n !== undefined && Number.isInteger(n) && n >= 0 ? n : undefined;
}

export function asBool(value: unknown): boolean {
  return value === true;
}

/** Items of an array, at most `max`; anything that is not an array gives `[]`. */
export function asArray(value: unknown, max: number): unknown[] {
  return Array.isArray(value) ? value.slice(0, max) : [];
}

/** Number array, at most `max` entries. One non-finite entry rejects the whole array (`[]`). */
export function asNumberArray(value: unknown, max: number): number[] {
  const out: number[] = [];
  for (const item of asArray(value, max)) {
    const n = asNumber(item);
    if (n === undefined) {
      return [];
    }
    out.push(n);
  }
  return out;
}

export interface LspPosition {
  readonly line: number;
  readonly character: number;
}

export interface LspRange {
  readonly start: LspPosition;
  readonly end: LspPosition;
}

/** A well-formed LSP range, else `undefined` (`range: null` means "no source site"). */
export function asRange(value: unknown): LspRange | undefined {
  if (!isRecord(value) || !isRecord(value.start) || !isRecord(value.end)) {
    return undefined;
  }
  const sl = asCount(value.start.line);
  const sc = asCount(value.start.character);
  const el = asCount(value.end.line);
  const ec = asCount(value.end.character);
  if (sl === undefined || sc === undefined || el === undefined || ec === undefined) {
    return undefined;
  }
  if (el < sl || (el === sl && ec < sc)) {
    return undefined;
  }
  return { start: { line: sl, character: sc }, end: { line: el, character: ec } };
}
