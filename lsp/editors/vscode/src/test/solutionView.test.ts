import * as assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { SolveResult, parseSolveResult } from '../pure/protocol';
import { constraintRows, formatNumber, hasDuals, infeasibilityTree, statusTone, summaryLines, variableRows } from '../pure/solutionView';

const range = { start: { line: 1, character: 0 }, end: { line: 1, character: 4 } };
const solved = parseSolveResult({
  version: 2,
  solver: { name: 'HiGHS', version: '1.11.0' },
  status: 'Optimal',
  objectiveValue: 12450.5,
  mipGap: 0.0125,
  timings: { buildMs: 3, solveMs: 420, extractMs: 1 },
  variables: [
    { name: 'beta', value: 0, reducedCost: -2, range },
    { name: 'alpha', value: 25, reducedCost: 0, range },
    { name: 'gamma', value: -100, reducedCost: 1.5 },
  ],
  constraints: [
    { name: 'cap', activity: 100, dual: 4.25, slack: 0, binding: true, range },
    { name: 'dem', activity: 5, dual: 0, slack: 7, binding: false, range },
    { name: 'box', activity: 1, dual: -9, slack: 0.5, binding: false },
  ],
}) as SolveResult;
const defaults = { filter: '', sort: 'model' as const, narrow: false };

describe('formatNumber', () => {
  it('trims to six significant digits', () => {
    assert.equal(formatNumber(12450.5), '12450.5');
    assert.equal(formatNumber(1 / 3), '0.333333');
    assert.equal(formatNumber(0), '0');
    assert.equal(formatNumber(-2.5), '-2.5');
  });

  it('uses exponent form for extreme magnitudes', () => {
    assert.equal(formatNumber(1.5e12), '1.5e+12');
    assert.equal(formatNumber(2.5e-7), '2.5e-7');
  });

  it('does not throw on non-finite values', () => {
    assert.equal(formatNumber(Infinity), 'Infinity');
    assert.equal(formatNumber(NaN), 'NaN');
  });
});

describe('statusTone', () => {
  it('maps HiGHS statuses to a tone', () => {
    assert.equal(statusTone('Optimal'), 'ok');
    assert.equal(statusTone('ReachedTimeLimit'), 'warn');
    assert.equal(statusTone('Infeasible'), 'error');
    assert.equal(statusTone('UnboundedOrInfeasible'), 'error');
  });
});

describe('tables', () => {
  it('keeps the source index so a click can find the range', () => {
    const rows = variableRows(solved, { ...defaults, sort: 'name' });
    assert.deepEqual(rows.map((r) => [r.variable.name, r.source]), [['alpha', 1], ['beta', 0], ['gamma', 2]]);
  });

  it('sorts variables by value and reduced cost magnitude', () => {
    assert.deepEqual(variableRows(solved, { ...defaults, sort: 'value' }).map((r) => r.variable.name), ['gamma', 'alpha', 'beta']);
    assert.deepEqual(variableRows(solved, { ...defaults, sort: 'dual' }).map((r) => r.variable.name), ['beta', 'gamma', 'alpha']);
  });

  it('hides zero values when narrowed and filters by name', () => {
    assert.deepEqual(variableRows(solved, { ...defaults, narrow: true }).map((r) => r.variable.name), ['alpha', 'gamma']);
    assert.deepEqual(variableRows(solved, { ...defaults, filter: 'MM' }).map((r) => r.variable.name), ['gamma']);
  });

  it('shows binding constraints only when narrowed', () => {
    assert.deepEqual(constraintRows(solved, { ...defaults, narrow: true }).map((r) => r.constraint.name), ['cap']);
  });

  it('sorts constraints by slack and by dual magnitude', () => {
    assert.deepEqual(constraintRows(solved, { ...defaults, sort: 'slack' }).map((r) => r.constraint.name), ['cap', 'box', 'dem']);
    assert.deepEqual(constraintRows(solved, { ...defaults, sort: 'dual' }).map((r) => r.constraint.name), ['box', 'cap', 'dem']);
  });

  it('detects whether duals exist (MIPs have none)', () => {
    assert.equal(hasDuals(solved), true);
    assert.equal(hasDuals({ ...solved, variables: [{ name: 'x', value: 1 }], constraints: [{ name: 'c', activity: 1, slack: 0, binding: true }] }), false);
  });
});

describe('summaryLines', () => {
  it('lists status, objective, gap, timings and solver', () => {
    assert.deepEqual(summaryLines(solved), [
      'Status: Optimal',
      'Objective: 12450.5',
      'MIP gap: 1.25%',
      'Time: build 3 ms, solve 420 ms, extract 1 ms',
      'Solver: HiGHS 1.11.0',
    ]);
  });
});

describe('infeasibilityTree', () => {
  const tree = infeasibilityTree({
    method: 'iis',
    totalViolation: 3,
    constraints: [{ name: 'a', side: 'lower', violation: 2, range }, { name: 'b', side: 'upper' }],
    bounds: [{ variable: 'x', side: 'upper', gap: 1, range }],
  });

  it('roots at the method and lists constraints then bounds', () => {
    assert.equal(tree.label, 'Irreducible infeasible subsystem (total violation 3)');
    assert.deepEqual(tree.children.map((c) => c.label), ['Constraints (2)', 'Variable bounds (1)']);
    assert.deepEqual(tree.children[0].children.map((c) => c.label), ['a', 'b']);
  });

  it('links only nodes that have a source range', () => {
    assert.equal(tree.children[0].children[0].group, 'infeasibility.constraints');
    assert.equal(tree.children[0].children[0].index, 0);
    assert.equal(tree.children[0].children[1].group, undefined);
    assert.equal(tree.children[1].children[0].group, 'infeasibility.bounds');
  });

  it('flags the elastic method as heuristic', () => {
    assert.match(infeasibilityTree({ method: 'elastic', constraints: [], bounds: [] }).label, /heuristic/);
  });
});
