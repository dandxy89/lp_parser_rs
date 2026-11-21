/** Custom LSP requests this extension drives (fleet contract 2.1). */
export const LP_REQUESTS = [
  'lp/model',
  'lp/explain',
  'lp/graph',
  'lp/matrix',
  'lp/diff',
  'lp/solve',
  'lp/solverInfo',
] as const;

export type LpRequest = (typeof LP_REQUESTS)[number];

/** The requests a running server advertised. */
export type LpCapabilities = ReadonlySet<LpRequest>;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isLpRequest(value: unknown): value is LpRequest {
  return typeof value === 'string' && (LP_REQUESTS as readonly string[]).includes(value);
}

/**
 * Reads `InitializeResult.capabilities.experimental` and returns the LP requests it advertises.
 * Missing, malformed or unknown entries are ignored, so bad input yields the empty set.
 */
export function parseLpCapabilities(experimental: unknown): LpCapabilities {
  const advertised = new Set<LpRequest>();
  if (!isRecord(experimental) || !isRecord(experimental.lp) || !Array.isArray(experimental.lp.requests)) {
    return advertised;
  }
  for (const request of experimental.lp.requests) {
    if (isLpRequest(request)) {
      advertised.add(request);
    }
  }
  return advertised;
}

/** Context key that gates commands for `request`, e.g. `lp/model` becomes `lp.supports.model`. */
export function supportsContextKey(request: LpRequest): string {
  return `lp.supports.${request.slice('lp/'.length)}`;
}
