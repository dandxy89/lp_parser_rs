export type ServerState = 'starting' | 'running' | 'failed' | 'stopped';

export interface DiagnosticCounts {
  readonly errors: number;
  readonly warnings: number;
}

function assertCount(name: string, value: number): void {
  if (!Number.isInteger(value) || value < 0) {
    throw new RangeError(`${name} must be a non-negative integer, got ${value}`);
  }
}

/** Model size from `lp/model`'s summary. */
export interface ModelStats {
  readonly objectives: number;
  readonly constraints: number;
  readonly variables: number;
}

/** Whether the model is valid: it has no error diagnostics. Unknown without counts. */
export function isValid(counts: DiagnosticCounts | undefined): boolean | undefined {
  return counts === undefined ? undefined : counts.errors === 0;
}

/** Compact size text such as `3 vars, 2 cons`; objectives are shown only when there is not exactly one. */
export function formatStats(stats: ModelStats): string {
  assertCount('objectives', stats.objectives);
  assertCount('constraints', stats.constraints);
  assertCount('variables', stats.variables);
  const parts = [`${stats.variables} vars`, `${stats.constraints} cons`];
  if (stats.objectives !== 1) {
    parts.push(`${stats.objectives} objs`);
  }
  return parts.join(', ');
}

/** Tooltip for the running state: size, validity and a hint. */
export function formatStatusTooltip(state: ServerState, counts?: DiagnosticCounts, stats?: ModelStats): string {
  const lines = [`LP language server (${state})`];
  if (stats !== undefined) {
    lines.push(`Model: ${formatStats(stats)}`);
  }
  const valid = isValid(counts);
  if (valid !== undefined) {
    lines.push(valid ? 'Valid: no errors' : `Invalid: ${counts?.errors ?? 0} error(s)`);
  }
  lines.push('Click to analyse the model.');
  return lines.join('\n');
}

/**
 * Status bar text for the server state. `counts` are the diagnostics for the active LP document;
 * omit them when no LP document is active.
 */
export function formatStatusText(state: ServerState, counts?: DiagnosticCounts, stats?: ModelStats): string {
  switch (state) {
    case 'starting':
      return '$(sync~spin) LP';
    case 'failed':
      return '$(error) LP: server failed';
    case 'stopped':
      return '$(circle-slash) LP: stopped';
    case 'running':
      if (counts === undefined) {
        return stats === undefined ? 'LP' : `LP ${formatStats(stats)}`;
      }
      assertCount('errors', counts.errors);
      assertCount('warnings', counts.warnings);
      const prefix = stats === undefined ? 'LP' : `LP ${formatStats(stats)}`;
      return `${prefix} $(${stats !== undefined && counts.errors === 0 ? 'check' : 'error'}) ${counts.errors} $(warning) ${counts.warnings}`;
  }
}
