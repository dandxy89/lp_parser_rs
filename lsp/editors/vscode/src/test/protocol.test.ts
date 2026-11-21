import * as assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  parseDiffResult,
  parseExplainResult,
  parseGraphResult,
  parseMatrixResult,
  parseModelNames,
  parseModelResult,
  parseSolveResult,
} from '../pure/protocol';
import { LIMITS } from '../pure/sanitise';

const range = { start: { line: 3, character: 0 }, end: { line: 3, character: 9 } };
const EVIL = '<img src=x onerror=alert(1)>‮';

describe('parseMatrixResult', () => {
  const entries = {
    uri: 'file:///m.lp',
    version: 3,
    mode: 'entries',
    shape: { rows: 2, cols: 2, nonzeros: 3 },
    rowOffset: 0,
    colOffset: 0,
    rows: [{ name: 'obj', kind: 'objective' }, { name: 'c1', kind: 'linear', class: 'normal', operator: '<=', rhs: 100 }],
    cols: [{ name: 'x', kind: 'continuous' }, { name: 'y', kind: 'binary' }],
    entries: { row: [0, 1, 1], col: [0, 0, 1], value: [1, 2, -3] },
    scale: { minAbs: 1, maxAbs: 3 },
    truncated: false,
  };

  it('reads an entries window', () => {
    const parsed = parseMatrixResult(entries);
    assert.equal(parsed?.mode, 'entries');
    if (parsed?.mode === 'entries') {
      assert.equal(parsed.version, 3);
      assert.deepEqual(parsed.entries.value, [1, 2, -3]);
      assert.equal(parsed.rows[1].operator, '<=');
      assert.equal(parsed.rows[1].rhs, 100);
      assert.equal(parsed.truncated, false);
    }
  });

  it('drops entries that point outside the window and reports truncation', () => {
    const parsed = parseMatrixResult({ ...entries, entries: { row: [0, 5, -1, 1], col: [0, 0, 0, 1], value: [1, 2, 3, 4] } });
    assert.ok(parsed?.mode === 'entries');
    if (parsed?.mode === 'entries') {
      assert.deepEqual(parsed.entries.row, [0, 1]);
      assert.deepEqual(parsed.entries.value, [1, 4]);
      assert.equal(parsed.truncated, true);
    }
  });

  it('uses the shorter of mismatched entry arrays', () => {
    const parsed = parseMatrixResult({ ...entries, entries: { row: [0, 1], col: [0], value: [1, 2] } });
    assert.ok(parsed?.mode === 'entries');
    if (parsed?.mode === 'entries') {
      assert.equal(parsed.entries.row.length, 1);
    }
  });

  it('reads a density result and rejects arrays of the wrong length', () => {
    const density = {
      mode: 'density',
      version: null,
      shape: { rows: 4, cols: 3, nonzeros: 10 },
      bins: { rows: 2, cols: 2 },
      rowEdges: [0, 2],
      colEdges: [0, 1],
      count: [2, 1, 1, 2],
      maxAbs: [5, 1, 2, 4],
      sign: [1, -1, -1, 0],
    };
    const parsed = parseMatrixResult(density);
    assert.equal(parsed?.mode, 'density');
    assert.equal(parsed?.version, null);
    assert.equal(parseMatrixResult({ ...density, count: [1, 2, 3] }), undefined);
    assert.equal(parseMatrixResult({ ...density, bins: { rows: LIMITS.bins + 1, cols: 1 } }), undefined);
    assert.equal(parseMatrixResult({ ...density, bins: { rows: 0, cols: 2 } }), undefined);
  });

  it('rejects garbage', () => {
    for (const bad of [null, 3, 'x', [], {}, { mode: 'entries' }, { mode: 'other', shape: { rows: 1, cols: 1, nonzeros: 0 } }]) {
      assert.equal(parseMatrixResult(bad), undefined);
    }
  });

  it('cleans names', () => {
    const parsed = parseMatrixResult({ ...entries, cols: [{ name: EVIL, kind: 'continuous' }, { name: 'y', kind: 'binary' }] });
    assert.ok(parsed?.mode === 'entries');
    if (parsed?.mode === 'entries') {
      assert.ok(!parsed.cols[0].name.includes('‮'));
    }
  });
});

describe('parseGraphResult', () => {
  const graph = {
    version: 3,
    nodes: [
      { id: 0, kind: 'constraint', name: 'c1', degree: 2, component: 0 },
      { id: 1, kind: 'variable', name: 'x', degree: 1, component: 0 },
      { id: 7, kind: 'variable', name: 'y', degree: 1, component: 0 },
    ],
    edges: { source: [0, 0], target: [1, 7], coefficient: [2, 3] },
    components: { count: 1, sizes: [3] },
    isolated: ['w'],
    hubs: [{ name: 'x', degree: 1 }],
    truncated: false,
  };

  it('maps ids to indices and keeps row-to-variable edges', () => {
    const parsed = parseGraphResult(graph);
    assert.ok(parsed);
    assert.deepEqual(parsed.edges.source, [0, 0]);
    assert.deepEqual(parsed.edges.target, [1, 2]);
    assert.deepEqual(parsed.edges.coefficient, [2, 3]);
    assert.deepEqual(parsed.isolated, ['w']);
  });

  it('drops edges with unknown ends, reversed direction or the wrong kinds', () => {
    const parsed = parseGraphResult({ ...graph, edges: { source: [0, 1, 0, 99], target: [1, 0, 0, 1], coefficient: [1, 1, 1, 1] } });
    assert.ok(parsed);
    assert.equal(parsed.edges.source.length, 1);
    assert.equal(parsed.truncated, true);
  });

  it('skips duplicate ids and unknown kinds', () => {
    const parsed = parseGraphResult({
      ...graph,
      nodes: [...graph.nodes, { id: 0, kind: 'variable', name: 'dup' }, { id: 9, kind: 'pizza', name: 'z' }],
    });
    assert.equal(parsed?.nodes.length, 3);
  });

  it('caps nodes', () => {
    const nodes = Array.from({ length: LIMITS.nodes + 10 }, (_, id) => ({ id, kind: 'variable', name: `v${id}`, degree: 0, component: 0 }));
    const parsed = parseGraphResult({ ...graph, nodes, edges: { source: [], target: [], coefficient: [] } });
    assert.equal(parsed?.nodes.length, LIMITS.nodes);
    assert.equal(parsed?.truncated, true);
  });

  it('rejects a missing edge object', () => {
    assert.equal(parseGraphResult({ nodes: [] }), undefined);
    assert.equal(parseGraphResult(null), undefined);
  });
});

describe('parseDiffResult', () => {
  const diff = {
    version: 3,
    label: 'HEAD',
    identical: false,
    sense: { old: 'Minimize', new: 'Maximize' },
    variables: { added: [{ name: 'x_new', range }], removed: [{ name: 'x_old' }], changed: [{ name: 'y', old: 'a', new: 'b', range }] },
    constraints: { added: [], removed: [], modified: [{ name: 'c1', changes: ['rhs 100 -> 120'], range }] },
    objectives: { added: [], removed: [], modified: [] },
  };

  it('reads all sections, mapping modified to changed', () => {
    const parsed = parseDiffResult(diff);
    assert.ok(parsed);
    assert.equal(parsed.label, 'HEAD');
    assert.equal(parsed.sense?.new, 'Maximize');
    assert.equal(parsed.variables.removed[0].range, undefined);
    assert.deepEqual(parsed.constraints.changed[0].changes, ['rhs 100 -> 120']);
  });

  it('cleans hostile names and caps rows', () => {
    const many = Array.from({ length: LIMITS.listRows + 5 }, (_, i) => ({ name: `${EVIL}${i}` }));
    const parsed = parseDiffResult({ ...diff, variables: { added: many, removed: [], changed: [] } });
    assert.ok(parsed);
    assert.equal(parsed.variables.added.length, LIMITS.listRows);
    assert.equal(parsed.truncated, true);
    assert.ok(!parsed.variables.added[0].name.includes('‮'));
  });

  it('rejects a missing section', () => {
    assert.equal(parseDiffResult({ ...diff, variables: null }), undefined);
  });
});

describe('parseSolveResult', () => {
  const solve = {
    version: 4,
    solver: { name: 'HiGHS', version: '1.11.0', helper: 'lp-highs 0.1.0' },
    status: 'Optimal',
    objectiveValue: 12450,
    timings: { buildMs: 3, solveMs: 420, extractMs: 1 },
    variables: [{ name: 'x1', value: 25, reducedCost: 0, range }, { name: 'x2', value: 0 }],
    constraints: [{ name: 'capacity', activity: 100, dual: 4.25, slack: 0, binding: true, range }],
    infeasibility: null,
    log: null,
  };

  it('reads an optimal result', () => {
    const parsed = parseSolveResult(solve);
    assert.ok(parsed);
    assert.equal(parsed.objectiveValue, 12450);
    assert.equal(parsed.variables[1].reducedCost, undefined);
    assert.equal(parsed.constraints[0].dual, 4.25);
    assert.equal(parsed.infeasibility, undefined);
    assert.equal(parsed.mipGap, undefined);
  });

  it('reads an infeasibility report', () => {
    const parsed = parseSolveResult({
      ...solve,
      status: 'Infeasible',
      variables: [],
      constraints: [],
      infeasibility: {
        method: 'iis',
        constraints: [{ name: 'a', side: 'lower', violation: 2, range }, { name: 'b', side: 'upper' }],
        bounds: [{ variable: 'x', side: 'upper', gap: 1 }],
        totalViolation: 3,
      },
    });
    assert.equal(parsed?.infeasibility?.method, 'iis');
    assert.equal(parsed?.infeasibility?.constraints.length, 2);
    assert.equal(parsed?.infeasibility?.bounds[0].gap, 1);
    assert.equal(parsed?.infeasibility?.totalViolation, 3);
  });

  it('drops rows with non-finite numbers and caps long lists', () => {
    const rows = Array.from({ length: LIMITS.listRows + 3 }, (_, i) => ({ name: `v${i}`, value: i }));
    const parsed = parseSolveResult({ ...solve, variables: [{ name: 'bad', value: 'NaN' }, ...rows] });
    assert.ok(parsed);
    assert.ok(parsed.variables.every((v) => v.name !== 'bad'));
    assert.equal(parsed.truncated, true);
  });

  it('caps the log and requires a status', () => {
    const parsed = parseSolveResult({ ...solve, log: 'l'.repeat(LIMITS.text * 2) });
    assert.equal(parsed?.log?.length, LIMITS.text);
    assert.equal(parseSolveResult({ ...solve, status: 3 }), undefined);
  });
});

describe('parseExplainResult', () => {
  const explain = {
    version: 3,
    kind: 'constraint',
    name: 'production_limit',
    range,
    markdown: '### production_limit\nLimits x',
    terms: [{ variable: 'x1', coefficient: 2, range }],
    termsTruncated: false,
    operator: '<=',
    rhs: 500,
    activity: { min: 0, max: null },
    related: [{ name: 'c7', kind: 'constraint', shared: 3 }],
    usage: [],
    diagnostics: ['proportional-row'],
  };

  it('reads a constraint explanation and keeps infinite activity as null', () => {
    const parsed = parseExplainResult(explain);
    assert.ok(parsed);
    assert.equal(parsed.activity?.max, null);
    assert.equal(parsed.activity?.min, 0);
    assert.equal(parsed.markdown, '### production_limit\nLimits x');
    assert.equal(parsed.related[0].shared, 3);
  });

  it('reads variable bounds with null for an infinite side', () => {
    const parsed = parseExplainResult({ kind: 'variable', name: 'x', bounds: { lower: 0, upper: null, variableKind: 'continuous' }, usage: [{ entity: 'c1', kind: 'constraint', role: 'linear', coefficient: 2 }] });
    assert.equal(parsed?.bounds?.upper, null);
    assert.equal(parsed?.usage[0].coefficient, 2);
  });

  it('strips controls from markdown but keeps its lines', () => {
    const parsed = parseExplainResult({ ...explain, markdown: 'a\u0000b\nc‮d\r\ne' });
    assert.equal(parsed?.markdown, 'ab\ncd\ne');
  });

  it('rejects an unknown kind', () => {
    assert.equal(parseExplainResult({ ...explain, kind: 'sos' }), undefined);
  });

  it('keeps a cleaned, capped description and drops non-strings and empty ones', () => {
    assert.equal(parseExplainResult({ ...explain, description: 'Plant\u202E cap' })?.description, 'Plant cap');
    const long = parseExplainResult({ ...explain, description: 'a'.repeat(LIMITS.description + 50) })?.description;
    assert.equal(long?.length, LIMITS.description);
    assert.ok(long?.endsWith('…'));
    for (const bad of [7, '', null, ['x']]) {
      assert.equal('description' in (parseExplainResult({ ...explain, description: bad }) ?? {}), false);
    }
    assert.equal('description' in (parseExplainResult(explain) ?? {}), false);
  });
});

describe('parseModelResult descriptions', () => {
  it('keeps entry descriptions', () => {
    const parsed = parseModelResult({
      summary: {},
      objectives: [{ name: 'o', description: 'Cost' }],
      constraints: [{ name: 'c', description: 'Cap\u202E' }, { name: 'd', description: 3 }],
      variables: [{ name: 'x', description: 'Width' }],
      truncated: {},
    });
    assert.equal(parsed?.objectives[0].description, 'Cost');
    assert.equal(parsed?.constraints[0].description, 'Cap');
    assert.equal('description' in (parsed?.constraints[1] ?? {}), false);
    assert.equal(parsed?.variables[0].description, 'Width');
  });
});

describe('parseModelNames', () => {
  it('collects names and ignores junk', () => {
    const parsed = parseModelNames({ constraints: [{ name: 'c1' }, 4, { name: 7 }], variables: [{ name: 'x' }], objectives: 'no' });
    assert.deepEqual(parsed, { constraints: ['c1'], variables: ['x'], objectives: [] });
    assert.equal(parseModelNames(null), undefined);
  });
});
