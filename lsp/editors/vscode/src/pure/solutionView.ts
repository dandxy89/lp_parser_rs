import { Infeasibility, SolveResult, SolvedConstraint, SolvedVariable } from './protocol';

/** Compact number: up to 6 significant digits, exponent form for very large or small magnitudes. */
export function formatNumber(value: number): string {
  if (!Number.isFinite(value)) {
    return String(value);
  }
  if (value === 0) {
    return '0';
  }
  const magnitude = Math.abs(value);
  if (magnitude >= 1e9 || magnitude < 1e-4) {
    return value.toExponential(4).replace(/\.?0+e/, 'e');
  }
  return String(Number(value.toPrecision(6)));
}

export type StatusTone = 'ok' | 'warn' | 'error';

/** Colour class for a HiGHS status name. */
export function statusTone(status: string): StatusTone {
  if (status === 'Optimal') {
    return 'ok';
  }
  if (status === 'ReachedTimeLimit' || status === 'ReachedInterrupt' || status === 'ObjectiveBound' || status === 'ObjectiveTarget') {
    return 'warn';
  }
  return 'error';
}

export type SortKey = 'model' | 'name' | 'value' | 'slack' | 'dual';

export interface TableOptions {
  readonly filter: string;
  readonly sort: SortKey;
  /** Variables: hide zero values. Constraints: show binding rows only. */
  readonly narrow: boolean;
}

export interface VariableRow {
  readonly source: number;
  readonly variable: SolvedVariable;
}

export interface ConstraintRow {
  readonly source: number;
  readonly constraint: SolvedConstraint;
}

const matches = (name: string, filter: string): boolean => {
  const needle = filter.trim().toLowerCase();
  return needle.length === 0 || name.toLowerCase().includes(needle);
};

/** Rows keep `source`, their index in the result, so a click can ask the host for the range. */
export function variableRows(result: SolveResult, options: TableOptions): VariableRow[] {
  const rows = result.variables
    .map((variable, source) => ({ source, variable }))
    .filter(({ variable }) => matches(variable.name, options.filter) && (!options.narrow || variable.value !== 0));
  if (options.sort === 'name') {
    rows.sort((a, b) => a.variable.name.localeCompare(b.variable.name) || a.source - b.source);
  } else if (options.sort === 'value') {
    rows.sort((a, b) => Math.abs(b.variable.value) - Math.abs(a.variable.value) || a.source - b.source);
  } else if (options.sort === 'dual') {
    rows.sort((a, b) => Math.abs(b.variable.reducedCost ?? 0) - Math.abs(a.variable.reducedCost ?? 0) || a.source - b.source);
  }
  return rows;
}

export function constraintRows(result: SolveResult, options: TableOptions): ConstraintRow[] {
  const rows = result.constraints
    .map((constraint, source) => ({ source, constraint }))
    .filter(({ constraint }) => matches(constraint.name, options.filter) && (!options.narrow || constraint.binding));
  if (options.sort === 'name') {
    rows.sort((a, b) => a.constraint.name.localeCompare(b.constraint.name) || a.source - b.source);
  } else if (options.sort === 'slack') {
    rows.sort((a, b) => a.constraint.slack - b.constraint.slack || a.source - b.source);
  } else if (options.sort === 'dual') {
    rows.sort((a, b) => Math.abs(b.constraint.dual ?? 0) - Math.abs(a.constraint.dual ?? 0) || a.source - b.source);
  } else if (options.sort === 'value') {
    rows.sort((a, b) => Math.abs(b.constraint.activity) - Math.abs(a.constraint.activity) || a.source - b.source);
  }
  return rows;
}

/** Whether the result carries duals (absent for MIPs). */
export function hasDuals(result: SolveResult): boolean {
  return result.constraints.some((c) => c.dual !== undefined) || result.variables.some((v) => v.reducedCost !== undefined);
}

/** Header lines: status, objective, gap, timings. */
export function summaryLines(result: SolveResult): string[] {
  const lines = [`Status: ${result.status}`];
  if (result.objectiveValue !== undefined) {
    lines.push(`Objective: ${formatNumber(result.objectiveValue)}`);
  }
  if (result.mipGap !== undefined) {
    lines.push(`MIP gap: ${formatNumber(result.mipGap * 100)}%`);
  }
  const { buildMs, solveMs, extractMs } = result.timings;
  lines.push(`Time: build ${buildMs} ms, solve ${solveMs} ms, extract ${extractMs} ms`);
  const solver = [result.solver.name, result.solver.version].filter(Boolean).join(' ');
  if (solver.length > 0) {
    lines.push(`Solver: ${solver}`);
  }
  return lines;
}

export interface TreeNode {
  readonly label: string;
  readonly detail?: string;
  /** `revealItem` address; `undefined` when the node has no source site. */
  readonly group?: string;
  readonly index?: number;
  readonly children: readonly TreeNode[];
}

/** The infeasibility tree: method at the root, then constraints and bounds that cannot hold together. */
export function infeasibilityTree(inf: Infeasibility): TreeNode {
  const constraints: TreeNode[] = inf.constraints.map((c, index) => ({
    label: c.name,
    detail: [c.side, c.violation !== undefined ? `violation ${formatNumber(c.violation)}` : ''].filter(Boolean).join(', '),
    ...(c.range !== undefined ? { group: 'infeasibility.constraints', index } : {}),
    children: [],
  }));
  const bounds: TreeNode[] = inf.bounds.map((b, index) => ({
    label: b.variable,
    detail: [`${b.side} bound`, b.gap !== undefined ? `gap ${formatNumber(b.gap)}` : ''].filter(Boolean).join(', '),
    ...(b.range !== undefined ? { group: 'infeasibility.bounds', index } : {}),
    children: [],
  }));
  const method = inf.method === 'iis' ? 'Irreducible infeasible subsystem' : 'Elastic relaxation (heuristic)';
  const total = inf.totalViolation !== undefined ? ` (total violation ${formatNumber(inf.totalViolation)})` : '';
  return {
    label: `${method}${total}`,
    children: [
      { label: `Constraints (${constraints.length})`, children: constraints },
      { label: `Variable bounds (${bounds.length})`, children: bounds },
    ],
  };
}
