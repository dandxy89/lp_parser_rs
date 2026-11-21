import * as assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { diffCounts, diffGroup, diffSummary, filterDiffRows, flattenDiff } from '../pure/diffView';
import { DiffResult, parseDiffResult } from '../pure/protocol';

const range = { start: { line: 1, character: 0 }, end: { line: 1, character: 4 } };
const diff = parseDiffResult({
  label: 'HEAD',
  identical: false,
  sense: { old: 'Minimize', new: 'Maximize' },
  variables: { added: [{ name: 'x_new', range }], removed: [{ name: 'x_old' }], changed: [{ name: 'y', old: 'Continuous', new: 'Integer', range }] },
  constraints: { added: [{ name: 'demand', range }], removed: [], modified: [{ name: 'c1', changes: ['rhs 100 -> 120', 'op <= -> >='], range }] },
  objectives: { added: [], removed: [], modified: [] },
}) as DiffResult;

describe('flattenDiff', () => {
  const rows = flattenDiff(diff);

  it('orders objectives, constraints, then variables; added, removed, changed', () => {
    assert.deepEqual(
      rows.map((r) => `${r.section}.${r.change}:${r.name}`),
      ['constraints.added:demand', 'constraints.changed:c1', 'variables.added:x_new', 'variables.removed:x_old', 'variables.changed:y'],
    );
  });

  it('describes changes and addresses rows by list and index', () => {
    const c1 = rows.find((r) => r.name === 'c1');
    assert.equal(c1?.detail, 'rhs 100 -> 120; op <= -> >=');
    assert.equal(c1?.group, diffGroup('constraints', 'changed'));
    assert.equal(c1?.index, 0);
    assert.equal(rows.find((r) => r.name === 'y')?.detail, 'Continuous → Integer');
  });

  it('marks removed items as not navigable', () => {
    assert.equal(rows.find((r) => r.name === 'x_old')?.navigable, false);
    assert.equal(rows.find((r) => r.name === 'x_new')?.navigable, true);
  });
});

describe('filterDiffRows', () => {
  const rows = flattenDiff(diff);
  const all = new Set(['added', 'removed', 'changed'] as const);

  it('filters by change kind', () => {
    assert.equal(filterDiffRows(rows, { text: '', changes: new Set(['added']) }).length, 2);
    assert.equal(filterDiffRows(rows, { text: '', changes: new Set() }).length, 0);
  });

  it('filters by name or detail, case-insensitively', () => {
    assert.deepEqual(filterDiffRows(rows, { text: 'X_', changes: all }).map((r) => r.name), ['x_new', 'x_old']);
    assert.deepEqual(filterDiffRows(rows, { text: 'rhs 100', changes: all }).map((r) => r.name), ['c1']);
  });
});

describe('summaries', () => {
  it('counts per section', () => {
    assert.deepEqual(diffCounts(diff).variables, { added: 1, removed: 1, changed: 1 });
    assert.deepEqual(diffCounts(diff).objectives, { added: 0, removed: 0, changed: 0 });
  });

  it('summarises changes and the sense flip', () => {
    assert.equal(diffSummary(diff), 'sense Minimize → Maximize, +1 ~1 constraints, +1 -1 ~1 variables');
  });

  it('says so when identical or within tolerance', () => {
    assert.equal(diffSummary({ ...diff, identical: true }), 'The models are identical.');
    const none = { ...diff, sense: undefined, variables: { added: [], removed: [], changed: [] }, constraints: { added: [], removed: [], changed: [] } };
    assert.equal(diffSummary(none), 'No differences within tolerance.');
  });
});
