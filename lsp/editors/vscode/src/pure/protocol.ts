/**
 * Typed, defensive readers for the `lp/*` results (fleet contract section 2).
 *
 * Every reader takes `unknown`, returns `undefined` when the shape is unusable, and otherwise returns a
 * copy with names stripped of control/bidi characters and every collection capped (see `LIMITS`).
 * Nothing here trusts the server: the extension also reads models written by strangers.
 */
import {
  LIMITS,
  LspRange,
  asArray,
  asBool,
  asCount,
  asNumber,
  asNumberArray,
  asRange,
  cleanName,
  cleanText,
  isRecord,
} from './sanitise';

export type EntityKind = 'constraint' | 'objective' | 'variable';

export function asEntityKind(value: unknown): EntityKind | undefined {
  return value === 'constraint' || value === 'objective' || value === 'variable' ? value : undefined;
}

/** `version` is a number for an open document and `null` for an indexed one. */
function asVersion(value: unknown): number | null {
  return asNumber(value) ?? null;
}

function optionalNumber(record: Record<string, unknown>, key: string): number | undefined {
  return asNumber(record[key]);
}

// ---------------------------------------------------------------- lp/matrix

export interface MatrixRow {
  readonly name: string;
  readonly kind: string;
  readonly operator?: string;
  readonly rhs?: number;
}

export interface MatrixCol {
  readonly name: string;
  readonly kind: string;
}

export interface MatrixShape {
  readonly rows: number;
  readonly cols: number;
  readonly nonzeros: number;
}

export interface MatrixEntriesResult {
  readonly mode: 'entries';
  readonly version: number | null;
  readonly shape: MatrixShape;
  readonly rowOffset: number;
  readonly colOffset: number;
  readonly rows: readonly MatrixRow[];
  readonly cols: readonly MatrixCol[];
  /** Window-relative indices; invalid entries are dropped. */
  readonly entries: { readonly row: readonly number[]; readonly col: readonly number[]; readonly value: readonly number[] };
  readonly scale: { readonly minAbs: number; readonly maxAbs: number };
  readonly truncated: boolean;
}

export interface MatrixDensityResult {
  readonly mode: 'density';
  readonly version: number | null;
  readonly shape: MatrixShape;
  readonly bins: { readonly rows: number; readonly cols: number };
  readonly rowEdges: readonly number[];
  readonly colEdges: readonly number[];
  readonly count: readonly number[];
  readonly maxAbs: readonly number[];
  readonly sign: readonly number[];
}

export type MatrixResult = MatrixEntriesResult | MatrixDensityResult;

function parseShape(value: unknown): MatrixShape | undefined {
  if (!isRecord(value)) {
    return undefined;
  }
  const rows = asCount(value.rows);
  const cols = asCount(value.cols);
  const nonzeros = asCount(value.nonzeros);
  return rows === undefined || cols === undefined || nonzeros === undefined ? undefined : { rows, cols, nonzeros };
}

export function parseMatrixResult(raw: unknown): MatrixResult | undefined {
  if (!isRecord(raw)) {
    return undefined;
  }
  const shape = parseShape(raw.shape);
  if (shape === undefined) {
    return undefined;
  }
  if (raw.mode === 'density') {
    return parseDensity(raw, shape);
  }
  if (raw.mode !== 'entries') {
    return undefined;
  }
  const rowOffset = asCount(raw.rowOffset) ?? 0;
  const colOffset = asCount(raw.colOffset) ?? 0;
  const rows = asArray(raw.rows, LIMITS.matrixAxis).map(
    (item): MatrixRow => {
      const record = isRecord(item) ? item : {};
      const operator = typeof record.operator === 'string' ? cleanText(record.operator, 3) : undefined;
      const rhs = optionalNumber(record, 'rhs');
      return {
        name: cleanName(record.name),
        kind: cleanText(record.kind, 32),
        ...(operator !== undefined ? { operator } : {}),
        ...(rhs !== undefined ? { rhs } : {}),
      };
    },
  );
  const cols = asArray(raw.cols, LIMITS.matrixAxis).map((item): MatrixCol => {
    const record = isRecord(item) ? item : {};
    return { name: cleanName(record.name), kind: cleanText(record.kind, 32) };
  });
  const entriesRaw = isRecord(raw.entries) ? raw.entries : {};
  const er = asNumberArray(entriesRaw.row, LIMITS.matrixEntries);
  const ec = asNumberArray(entriesRaw.col, LIMITS.matrixEntries);
  const ev = asNumberArray(entriesRaw.value, LIMITS.matrixEntries);
  const length = Math.min(er.length, ec.length, ev.length);
  const row: number[] = [];
  const col: number[] = [];
  const value: number[] = [];
  for (let i = 0; i < length; i += 1) {
    if (Number.isInteger(er[i]) && Number.isInteger(ec[i]) && er[i] >= 0 && ec[i] >= 0 && er[i] < rows.length && ec[i] < cols.length) {
      row.push(er[i]);
      col.push(ec[i]);
      value.push(ev[i]);
    }
  }
  const scaleRaw = isRecord(raw.scale) ? raw.scale : {};
  const minAbs = asNumber(scaleRaw.minAbs) ?? 0;
  const maxAbs = asNumber(scaleRaw.maxAbs) ?? 0;
  return {
    mode: 'entries',
    version: asVersion(raw.version),
    shape,
    rowOffset,
    colOffset,
    rows,
    cols,
    entries: { row, col, value },
    scale: { minAbs: Math.min(minAbs, maxAbs), maxAbs },
    truncated: asBool(raw.truncated) || length > row.length,
  };
}

function parseDensity(raw: Record<string, unknown>, shape: MatrixShape): MatrixDensityResult | undefined {
  const binsRaw = isRecord(raw.bins) ? raw.bins : {};
  const rows = asCount(binsRaw.rows);
  const cols = asCount(binsRaw.cols);
  if (rows === undefined || cols === undefined || rows === 0 || cols === 0 || rows > LIMITS.bins || cols > LIMITS.bins) {
    return undefined;
  }
  const cells = rows * cols;
  const count = asNumberArray(raw.count, cells);
  const maxAbs = asNumberArray(raw.maxAbs, cells);
  const sign = asNumberArray(raw.sign, cells);
  const rowEdges = asNumberArray(raw.rowEdges, rows);
  const colEdges = asNumberArray(raw.colEdges, cols);
  if (count.length !== cells || maxAbs.length !== cells || sign.length !== cells || rowEdges.length !== rows || colEdges.length !== cols) {
    return undefined;
  }
  return { mode: 'density', version: asVersion(raw.version), shape, bins: { rows, cols }, rowEdges, colEdges, count, maxAbs, sign };
}

// ---------------------------------------------------------------- lp/graph

export type GraphNodeKind = 'constraint' | 'objective' | 'variable';

export interface GraphNode {
  readonly id: number;
  readonly kind: GraphNodeKind;
  readonly name: string;
  readonly degree: number;
  readonly component: number;
}

export interface GraphResult {
  readonly version: number | null;
  readonly nodes: readonly GraphNode[];
  /** Edge `i` joins `nodes[source[i]]` (row or objective) to `nodes[target[i]]` (variable); indices, not ids. */
  readonly edges: { readonly source: readonly number[]; readonly target: readonly number[]; readonly coefficient: readonly number[] };
  readonly components: { readonly count: number; readonly sizes: readonly number[] };
  readonly isolated: readonly string[];
  readonly hubs: readonly { readonly name: string; readonly degree: number }[];
  readonly truncated: boolean;
}

export function parseGraphResult(raw: unknown): GraphResult | undefined {
  if (!isRecord(raw) || !Array.isArray(raw.nodes) || !isRecord(raw.edges)) {
    return undefined;
  }
  const nodes: GraphNode[] = [];
  const indexOfId = new Map<number, number>();
  for (const item of asArray(raw.nodes, LIMITS.nodes)) {
    if (!isRecord(item)) {
      continue;
    }
    const id = asCount(item.id);
    const kind = asEntityKind(item.kind);
    if (id === undefined || kind === undefined || indexOfId.has(id)) {
      continue;
    }
    indexOfId.set(id, nodes.length);
    nodes.push({ id, kind, name: cleanName(item.name), degree: asCount(item.degree) ?? 0, component: asCount(item.component) ?? 0 });
  }
  const sourceIds = asNumberArray(raw.edges.source, LIMITS.edges);
  const targetIds = asNumberArray(raw.edges.target, LIMITS.edges);
  const coefficients = asNumberArray(raw.edges.coefficient, LIMITS.edges);
  const length = Math.min(sourceIds.length, targetIds.length);
  const source: number[] = [];
  const target: number[] = [];
  const coefficient: number[] = [];
  for (let i = 0; i < length; i += 1) {
    const s = indexOfId.get(sourceIds[i]);
    const t = indexOfId.get(targetIds[i]);
    if (s !== undefined && t !== undefined && nodes[s].kind !== 'variable' && nodes[t].kind === 'variable') {
      source.push(s);
      target.push(t);
      coefficient.push(coefficients[i] ?? 0);
    }
  }
  const componentsRaw = isRecord(raw.components) ? raw.components : {};
  const hubs = asArray(raw.hubs, 20).flatMap((item) =>
    isRecord(item) ? [{ name: cleanName(item.name), degree: asCount(item.degree) ?? 0 }] : [],
  );
  return {
    version: asVersion(raw.version),
    nodes,
    edges: { source, target, coefficient },
    components: {
      count: asCount(componentsRaw.count) ?? 0,
      sizes: asNumberArray(componentsRaw.sizes, LIMITS.components),
    },
    isolated: asArray(raw.isolated, 1000).map(cleanName),
    hubs,
    truncated: asBool(raw.truncated) || length > source.length || nodes.length < asArray(raw.nodes, LIMITS.nodes + 1).length,
  };
}

// ---------------------------------------------------------------- lp/diff

export interface DiffItem {
  readonly name: string;
  readonly range?: LspRange;
  /** Variable changes: old and new descriptions. */
  readonly old?: string;
  readonly new?: string;
  /** Constraint and objective changes: one line each. */
  readonly changes?: readonly string[];
}

export type DiffSectionName = 'variables' | 'constraints' | 'objectives';
export type DiffChangeKind = 'added' | 'removed' | 'changed';

export interface DiffSection {
  readonly added: readonly DiffItem[];
  readonly removed: readonly DiffItem[];
  /** `changed` for variables, `modified` for constraints and objectives. */
  readonly changed: readonly DiffItem[];
}

export interface DiffResult {
  readonly version: number | null;
  readonly label: string;
  readonly identical: boolean;
  readonly sense?: { readonly old: string; readonly new: string };
  readonly variables: DiffSection;
  readonly constraints: DiffSection;
  readonly objectives: DiffSection;
  /** Rows dropped by the per-list cap, per section. */
  readonly truncated: boolean;
}

function parseDiffItems(value: unknown, state: { truncated: boolean }): DiffItem[] {
  const all = Array.isArray(value) ? value : [];
  if (all.length > LIMITS.listRows) {
    state.truncated = true;
  }
  return all.slice(0, LIMITS.listRows).flatMap((item): DiffItem[] => {
    if (!isRecord(item)) {
      return [];
    }
    const range = asRange(item.range);
    const changes = Array.isArray(item.changes)
      ? item.changes.slice(0, LIMITS.changesPerRow).map((c) => cleanText(c, 512))
      : undefined;
    return [
      {
        name: cleanName(item.name),
        ...(range !== undefined ? { range } : {}),
        ...(typeof item.old === 'string' ? { old: cleanText(item.old, 512) } : {}),
        ...(typeof item.new === 'string' ? { new: cleanText(item.new, 512) } : {}),
        ...(changes !== undefined ? { changes } : {}),
      },
    ];
  });
}

function parseDiffSection(value: unknown, changedKey: 'changed' | 'modified', state: { truncated: boolean }): DiffSection {
  const record = isRecord(value) ? value : {};
  return {
    added: parseDiffItems(record.added, state),
    removed: parseDiffItems(record.removed, state),
    changed: parseDiffItems(record[changedKey], state),
  };
}

export function parseDiffResult(raw: unknown): DiffResult | undefined {
  if (!isRecord(raw) || !isRecord(raw.variables) || !isRecord(raw.constraints) || !isRecord(raw.objectives)) {
    return undefined;
  }
  const state = { truncated: false };
  const sense =
    isRecord(raw.sense) && typeof raw.sense.old === 'string' && typeof raw.sense.new === 'string'
      ? { old: cleanText(raw.sense.old, 32), new: cleanText(raw.sense.new, 32) }
      : undefined;
  return {
    version: asVersion(raw.version),
    label: cleanName(raw.label),
    identical: asBool(raw.identical),
    ...(sense !== undefined ? { sense } : {}),
    variables: parseDiffSection(raw.variables, 'changed', state),
    constraints: parseDiffSection(raw.constraints, 'modified', state),
    objectives: parseDiffSection(raw.objectives, 'modified', state),
    truncated: state.truncated,
  };
}

// ---------------------------------------------------------------- lp/solve

export interface SolvedVariable {
  readonly name: string;
  readonly value: number;
  readonly reducedCost?: number;
  readonly range?: LspRange;
}

export interface SolvedConstraint {
  readonly name: string;
  readonly activity: number;
  readonly slack: number;
  readonly dual?: number;
  readonly binding: boolean;
  readonly range?: LspRange;
}

export interface InfeasibleConstraint {
  readonly name: string;
  readonly side: string;
  readonly violation?: number;
  readonly range?: LspRange;
}

export interface InfeasibleBound {
  readonly variable: string;
  readonly side: string;
  readonly gap?: number;
  readonly range?: LspRange;
}

export interface Infeasibility {
  readonly method: string;
  readonly constraints: readonly InfeasibleConstraint[];
  readonly bounds: readonly InfeasibleBound[];
  readonly totalViolation?: number;
}

export interface SolveResult {
  readonly version: number | null;
  readonly solver: { readonly name: string; readonly version: string };
  readonly status: string;
  readonly objectiveValue?: number;
  readonly mipGap?: number;
  readonly timings: { readonly buildMs: number; readonly solveMs: number; readonly extractMs: number };
  readonly variables: readonly SolvedVariable[];
  readonly constraints: readonly SolvedConstraint[];
  readonly infeasibility?: Infeasibility;
  readonly log?: string;
  readonly truncated: boolean;
}

function withRange(record: Record<string, unknown>): { range?: LspRange } {
  const range = asRange(record.range);
  return range !== undefined ? { range } : {};
}

function withNumber<K extends string>(record: Record<string, unknown>, key: K): { [P in K]?: number } {
  const value = asNumber(record[key]);
  return (value !== undefined ? { [key]: value } : {}) as { [P in K]?: number };
}

export function parseSolveResult(raw: unknown): SolveResult | undefined {
  if (!isRecord(raw) || typeof raw.status !== 'string') {
    return undefined;
  }
  const solverRaw = isRecord(raw.solver) ? raw.solver : {};
  const timingsRaw = isRecord(raw.timings) ? raw.timings : {};
  const variablesAll = asArray(raw.variables, LIMITS.listRows + 1);
  const constraintsAll = asArray(raw.constraints, LIMITS.listRows + 1);
  const variables = variablesAll.slice(0, LIMITS.listRows).flatMap((item): SolvedVariable[] => {
    if (!isRecord(item)) {
      return [];
    }
    const value = asNumber(item.value);
    return value === undefined ? [] : [{ name: cleanName(item.name), value, ...withNumber(item, 'reducedCost'), ...withRange(item) }];
  });
  const constraints = constraintsAll.slice(0, LIMITS.listRows).flatMap((item): SolvedConstraint[] => {
    if (!isRecord(item)) {
      return [];
    }
    const activity = asNumber(item.activity);
    const slack = asNumber(item.slack);
    if (activity === undefined || slack === undefined) {
      return [];
    }
    return [{ name: cleanName(item.name), activity, slack, binding: asBool(item.binding), ...withNumber(item, 'dual'), ...withRange(item) }];
  });
  const log = typeof raw.log === 'string' ? cleanText(raw.log.replace(/\r\n?/g, '\n'), LIMITS.text) : undefined;
  return {
    version: asVersion(raw.version),
    solver: { name: cleanName(solverRaw.name), version: cleanText(solverRaw.version, 32) },
    status: cleanText(raw.status, 64),
    ...withNumber(raw, 'objectiveValue'),
    ...withNumber(raw, 'mipGap'),
    timings: {
      buildMs: asNumber(timingsRaw.buildMs) ?? 0,
      solveMs: asNumber(timingsRaw.solveMs) ?? 0,
      extractMs: asNumber(timingsRaw.extractMs) ?? 0,
    },
    variables,
    constraints,
    ...(isRecord(raw.infeasibility) ? { infeasibility: parseInfeasibility(raw.infeasibility) } : {}),
    ...(log !== undefined ? { log } : {}),
    truncated: variablesAll.length > LIMITS.listRows || constraintsAll.length > LIMITS.listRows,
  };
}

function parseInfeasibility(raw: Record<string, unknown>): Infeasibility {
  const constraints = asArray(raw.constraints, LIMITS.listRows).flatMap((item): InfeasibleConstraint[] =>
    isRecord(item)
      ? [{ name: cleanName(item.name), side: cleanText(item.side, 16), ...withNumber(item, 'violation'), ...withRange(item) }]
      : [],
  );
  const bounds = asArray(raw.bounds, LIMITS.listRows).flatMap((item): InfeasibleBound[] =>
    isRecord(item)
      ? [{ variable: cleanName(item.variable), side: cleanText(item.side, 16), ...withNumber(item, 'gap'), ...withRange(item) }]
      : [],
  );
  return { method: cleanText(raw.method, 16), constraints, bounds, ...withNumber(raw, 'totalViolation') };
}

// ---------------------------------------------------------------- lp/explain

export interface ExplainTerm {
  readonly variable: string;
  readonly coefficient: number;
  readonly range?: LspRange;
}

export interface ExplainResult {
  readonly kind: EntityKind;
  readonly name: string;
  readonly range?: LspRange;
  readonly markdown: string;
  readonly terms: readonly ExplainTerm[];
  readonly termsTruncated: boolean;
  readonly operator?: string;
  readonly rhs?: number;
  /** `null` side = infinite. */
  readonly activity?: { readonly min: number | null; readonly max: number | null };
  readonly related: readonly { readonly name: string; readonly kind: string; readonly shared: number }[];
  readonly usage: readonly { readonly entity: string; readonly kind: string; readonly role: string; readonly coefficient?: number; readonly range?: LspRange }[];
  readonly bounds?: { readonly lower: number | null; readonly upper: number | null; readonly variableKind: string };
  readonly diagnostics: readonly string[];
}

export function parseExplainResult(raw: unknown): ExplainResult | undefined {
  if (!isRecord(raw)) {
    return undefined;
  }
  const kind = asEntityKind(raw.kind);
  if (kind === undefined) {
    return undefined;
  }
  const terms = asArray(raw.terms, LIMITS.terms).flatMap((item): ExplainTerm[] => {
    if (!isRecord(item)) {
      return [];
    }
    const coefficient = asNumber(item.coefficient);
    return coefficient === undefined ? [] : [{ variable: cleanName(item.variable), coefficient, ...withRange(item) }];
  });
  const range = asRange(raw.range);
  const activity = isRecord(raw.activity)
    ? { min: asNumber(raw.activity.min) ?? null, max: asNumber(raw.activity.max) ?? null }
    : undefined;
  const bounds = isRecord(raw.bounds)
    ? { lower: asNumber(raw.bounds.lower) ?? null, upper: asNumber(raw.bounds.upper) ?? null, variableKind: cleanText(raw.bounds.variableKind, 32) }
    : undefined;
  const operator = typeof raw.operator === 'string' ? cleanText(raw.operator, 3) : undefined;
  const rhs = asNumber(raw.rhs);
  return {
    kind,
    name: cleanName(raw.name),
    ...(range !== undefined ? { range } : {}),
    markdown: cleanTextKeepNewlines(raw.markdown, LIMITS.text),
    terms,
    termsTruncated: asBool(raw.termsTruncated),
    ...(operator !== undefined ? { operator } : {}),
    ...(rhs !== undefined ? { rhs } : {}),
    ...(activity !== undefined ? { activity } : {}),
    related: asArray(raw.related, LIMITS.related).flatMap((item) =>
      isRecord(item) ? [{ name: cleanName(item.name), kind: cleanText(item.kind, 16), shared: asCount(item.shared) ?? 0 }] : [],
    ),
    usage: asArray(raw.usage, LIMITS.terms).flatMap((item) => {
      if (!isRecord(item)) {
        return [];
      }
      const coefficient = asNumber(item.coefficient);
      return [
        {
          entity: cleanName(item.entity),
          kind: cleanText(item.kind, 16),
          role: cleanText(item.role, 16),
          ...(coefficient !== undefined ? { coefficient } : {}),
          ...withRange(item),
        },
      ];
    }),
    ...(bounds !== undefined ? { bounds } : {}),
    diagnostics: asArray(raw.diagnostics, 100).map((d) => cleanText(d, 64)),
  };
}

/** Like `cleanText` but keeps newlines (markdown is multi-line). Other controls are still stripped. */
function cleanTextKeepNewlines(value: unknown, max: number): string {
  if (typeof value !== 'string') {
    return '';
  }
  return value
    .split(/\r\n|\r|\n/)
    .map((line) => cleanText(line, max))
    .join('\n')
    .slice(0, max);
}

// ---------------------------------------------------------------- lp/model (names only: quick picks)

export interface ModelNames {
  readonly constraints: readonly string[];
  readonly variables: readonly string[];
  readonly objectives: readonly string[];
}

export function parseModelNames(raw: unknown): ModelNames | undefined {
  if (!isRecord(raw)) {
    return undefined;
  }
  const names = (value: unknown): string[] =>
    asArray(value, LIMITS.listRows * 10).flatMap((item) => (isRecord(item) && typeof item.name === 'string' ? [cleanName(item.name)] : []));
  return { constraints: names(raw.constraints), variables: names(raw.variables), objectives: names(raw.objectives) };
}

// ---------------------------------------------------------------- lp/model (Model Explorer)

export interface ModelEntry {
  readonly name: string;
  readonly kind: string;
  readonly generated: boolean;
  readonly operator?: string;
  readonly rhs?: number;
  readonly bounds?: { readonly lower: number | null; readonly upper: number | null };
  readonly unused: boolean;
  readonly range?: LspRange;
}

export interface ModelResult {
  readonly version: number | null;
  readonly summary: {
    readonly sense: string;
    readonly objectives: number;
    readonly constraints: number;
    readonly variables: number;
    readonly nonzeros: number;
    readonly errors: number;
    readonly warnings: number;
    readonly infos: number;
  };
  readonly objectives: readonly ModelEntry[];
  readonly constraints: readonly ModelEntry[];
  readonly variables: readonly ModelEntry[];
  readonly truncated: boolean;
}

function parseModelEntries(value: unknown, kindDefault: string): ModelEntry[] {
  return asArray(value, LIMITS.listRows).flatMap((item): ModelEntry[] => {
    if (!isRecord(item) || typeof item.name !== 'string') {
      return [];
    }
    const bounds = isRecord(item.bounds)
      ? { lower: asNumber(item.bounds.lower) ?? null, upper: asNumber(item.bounds.upper) ?? null }
      : undefined;
    const operator = typeof item.operator === 'string' ? cleanText(item.operator, 3) : undefined;
    const rhs = asNumber(item.rhs);
    const range = asRange(item.range);
    return [
      {
        name: cleanName(item.name),
        kind: typeof item.kind === 'string' ? cleanText(item.kind, 32) : kindDefault,
        generated: asBool(item.generated),
        ...(operator !== undefined ? { operator } : {}),
        ...(rhs !== undefined ? { rhs } : {}),
        ...(bounds !== undefined ? { bounds } : {}),
        unused: asBool(item.unused),
        ...(range !== undefined ? { range } : {}),
      },
    ];
  });
}

export function parseModelResult(raw: unknown): ModelResult | undefined {
  if (!isRecord(raw) || !isRecord(raw.summary)) {
    return undefined;
  }
  const s = raw.summary;
  const d = isRecord(s.diagnostics) ? s.diagnostics : {};
  const truncated = isRecord(raw.truncated) ? raw.truncated : {};
  return {
    version: asVersion(raw.version),
    summary: {
      sense: cleanText(s.sense, 16),
      objectives: asCount(s.objectives) ?? 0,
      constraints: asCount(s.constraints) ?? 0,
      variables: asCount(s.variables) ?? 0,
      nonzeros: asCount(s.nonzeros) ?? 0,
      errors: asCount(d.error) ?? 0,
      warnings: asCount(d.warning) ?? 0,
      infos: asCount(d.info) ?? 0,
    },
    objectives: parseModelEntries(raw.objectives, 'objective'),
    constraints: parseModelEntries(raw.constraints, 'linear'),
    variables: parseModelEntries(raw.variables, 'continuous'),
    truncated: asBool(truncated.objectives) || asBool(truncated.constraints) || asBool(truncated.variables),
  };
}
