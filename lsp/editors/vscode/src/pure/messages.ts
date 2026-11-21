import { EntityKind, asEntityKind } from './protocol';
import { LIMITS, asCount, isRecord } from './sanitise';

/** Messages a webview may post to the extension host. Everything else is dropped. */
export type WebviewMessage =
  | { readonly type: 'ready' }
  | { readonly type: 'refresh' }
  /** Open the source of a named entity (the host resolves the range with `lp/explain`). */
  | { readonly type: 'reveal'; readonly kind: EntityKind; readonly name: string }
  /** Open the source of an item the host holds a range for, addressed by list and index. */
  | { readonly type: 'revealItem'; readonly group: string; readonly index: number }
  | { readonly type: 'explain'; readonly kind: EntityKind; readonly name: string }
  | { readonly type: 'window'; readonly rowOffset: number; readonly colOffset: number }
  | { readonly type: 'mode'; readonly mode: 'entries' | 'density' }
  | { readonly type: 'focus'; readonly kind: EntityKind; readonly name: string; readonly depth: 1 | 2 | 3 }
  | { readonly type: 'clearFocus' };

const GROUP = /^[A-Za-z]{1,24}(\.[A-Za-z]{1,24}){0,2}$/;
const MAX_INDEX = 10_000_000;

/** Validates an untrusted `postMessage` payload. */
export function parseWebviewMessage(raw: unknown): WebviewMessage | undefined {
  if (!isRecord(raw) || typeof raw.type !== 'string') {
    return undefined;
  }
  switch (raw.type) {
    case 'ready':
      return { type: 'ready' };
    case 'refresh':
      return { type: 'refresh' };
    case 'clearFocus':
      return { type: 'clearFocus' };
    case 'reveal':
    case 'explain': {
      const kind = asEntityKind(raw.kind);
      const name = entityName(raw.name);
      return kind !== undefined && name !== undefined ? { type: raw.type, kind, name } : undefined;
    }
    case 'revealItem': {
      const index = asCount(raw.index);
      return typeof raw.group === 'string' && GROUP.test(raw.group) && index !== undefined && index < MAX_INDEX
        ? { type: 'revealItem', group: raw.group, index }
        : undefined;
    }
    case 'window': {
      const rowOffset = asCount(raw.rowOffset);
      const colOffset = asCount(raw.colOffset);
      return rowOffset !== undefined && colOffset !== undefined && rowOffset < MAX_INDEX && colOffset < MAX_INDEX
        ? { type: 'window', rowOffset, colOffset }
        : undefined;
    }
    case 'mode':
      return raw.mode === 'entries' || raw.mode === 'density' ? { type: 'mode', mode: raw.mode } : undefined;
    case 'focus': {
      const kind = asEntityKind(raw.kind);
      const name = entityName(raw.name);
      const depth = raw.depth === 1 || raw.depth === 2 || raw.depth === 3 ? raw.depth : undefined;
      return kind !== undefined && name !== undefined && depth !== undefined ? { type: 'focus', kind, name, depth } : undefined;
    }
    default:
      return undefined;
  }
}

function entityName(value: unknown): string | undefined {
  return typeof value === 'string' && value.length > 0 && value.length <= LIMITS.name ? value : undefined;
}
