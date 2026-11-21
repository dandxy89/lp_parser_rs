import { isRecord } from './sanitise';

export const CONVERT_FROM_MPS = 'lp.convertFromMps';
/** The server's limit on MPS text; the client checks it first so the user gets the message without a round trip. */
export const MAX_MPS_BYTES = 64 * 1024 * 1024;

/** `{ lp, omitted }` from the server; undefined unless `lp` is a string. Non-string `omitted` entries are dropped; absent gives []. */
export function parseConvertFromMpsResult(raw: unknown): { lp: string; omitted: string[] } | undefined {
  if (!isRecord(raw) || typeof raw.lp !== 'string') {
    return undefined;
  }
  const omitted = Array.isArray(raw.omitted) ? raw.omitted.filter((entry): entry is string => typeof entry === 'string') : [];
  return { lp: raw.lp, omitted };
}
