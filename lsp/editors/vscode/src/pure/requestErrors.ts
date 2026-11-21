import { cleanText, isRecord } from './sanitise';

/** Failure kinds of `-32803 RequestFailed` (fleet contract 2.1). */
const KINDS = ['parseError', 'unsupportedModel', 'solverUnavailable', 'solverFailed', 'tooLarge'] as const;
export type FailureKind = (typeof KINDS)[number];

export const REQUEST_CANCELLED = -32800;
export const REQUEST_FAILED = -32803;
export const INVALID_PARAMS = -32602;

/** Whether `error` is the server (or the user) cancelling the request. */
export function isCancellation(error: unknown): boolean {
  return isRecord(error) && error.code === REQUEST_CANCELLED;
}

/**
 * One user-facing sentence for a failed `lp/*` request. Server text is passed through `cleanText`
 * because it can echo model content.
 */
export function describeRequestError(error: unknown): string {
  if (!isRecord(error)) {
    return typeof error === 'string' ? cleanText(error, 300) : 'The request failed.';
  }
  const message = cleanText(error.message, 300);
  const data = isRecord(error.data) ? error.data : undefined;
  const kind = data !== undefined && typeof data.kind === 'string' ? data.kind : undefined;
  const detail = data !== undefined ? cleanText(data.message, 300) : '';
  switch (kind as FailureKind | undefined) {
    case 'parseError': {
      const side = data !== undefined && (data.side === 'current' || data.side === 'against') ? data.side : undefined;
      const where = side === 'against' ? 'the comparison model' : 'this model';
      return `Could not parse ${where}: ${detail || message}`;
    }
    case 'unsupportedModel':
      return `The solver does not support this model: ${detail || message}`;
    case 'solverUnavailable': {
      const reason = data !== undefined ? cleanText(data.reason, 300) : '';
      return `No solver is available: ${reason || detail || message}. Set \`lp.solver.path\` or install lp-highs.`;
    }
    case 'solverFailed':
      return `The solver failed: ${detail || message}`;
    case 'tooLarge':
      return `The model is too large for this request: ${detail || message}`;
    default:
      break;
  }
  if (error.code === INVALID_PARAMS) {
    return `The server rejected the request: ${message}`;
  }
  return message.length > 0 ? message : 'The request failed.';
}
