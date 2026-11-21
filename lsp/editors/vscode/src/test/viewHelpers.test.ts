import * as assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { explainFacts, formatBounds } from '../pure/explainView';
import { DiffResult, ExplainResult, SolveResult, parseDiffResult, parseExplainResult, parseSolveResult } from '../pure/protocol';
import { diffRange, explainRange, solutionRange } from '../pure/ranges';
import { INVALID_PARAMS, REQUEST_CANCELLED, describeRequestError, isCancellation } from '../pure/requestErrors';

const range = { start: { line: 2, character: 1 }, end: { line: 2, character: 8 } };

describe('ranges', () => {
  const diff = parseDiffResult({
    variables: { added: [{ name: 'a', range }], removed: [{ name: 'r' }], changed: [] },
    constraints: { added: [], removed: [], modified: [{ name: 'c', range }] },
    objectives: { added: [], removed: [], modified: [] },
  }) as DiffResult;
  const solve = parseSolveResult({
    status: 'Infeasible',
    variables: [{ name: 'x', value: 1, range }],
    constraints: [{ name: 'c', activity: 1, slack: 0, binding: true }],
    infeasibility: { method: 'iis', constraints: [{ name: 'k', side: 'lower', range }], bounds: [{ variable: 'x', side: 'upper', range }] },
  }) as SolveResult;
  const explain = parseExplainResult({ kind: 'variable', name: 'x', range }) as ExplainResult;

  it('resolves diff groups, and refuses removed items, unknown groups and bad indices', () => {
    assert.deepEqual(diffRange(diff, 'variables.added', 0), range);
    assert.deepEqual(diffRange(diff, 'constraints.changed', 0), range);
    assert.equal(diffRange(diff, 'variables.removed', 0), undefined);
    assert.equal(diffRange(diff, 'variables.added', 5), undefined);
    assert.equal(diffRange(diff, 'constructor.prototype', 0), undefined);
    assert.equal(diffRange(diff, '__proto__.added', 0), undefined);
    assert.equal(diffRange(diff, 'variables', 0), undefined);
  });

  it('resolves solution groups', () => {
    assert.deepEqual(solutionRange(solve, 'variables', 0), range);
    assert.equal(solutionRange(solve, 'constraints', 0), undefined);
    assert.deepEqual(solutionRange(solve, 'infeasibility.constraints', 0), range);
    assert.deepEqual(solutionRange(solve, 'infeasibility.bounds', 0), range);
    assert.equal(solutionRange(solve, 'nope', 0), undefined);
  });

  it('resolves only the explained entity itself', () => {
    assert.deepEqual(explainRange(explain, 'explain', 0), range);
    assert.equal(explainRange(explain, 'explain', 1), undefined);
    assert.equal(explainRange(explain, 'variables', 0), undefined);
  });
});

describe('explainFacts', () => {
  it('writes infinite bounds as -inf / +inf', () => {
    assert.equal(formatBounds(null, 10), '-inf .. 10');
    assert.equal(formatBounds(0, null), '0 .. +inf');
  });

  it('lists operator and rhs, activity, bounds and diagnostics', () => {
    const constraint = parseExplainResult({ kind: 'constraint', name: 'c', operator: '<=', rhs: 500, activity: { min: 0, max: null }, diagnostics: ['proportional-row'] }) as ExplainResult;
    assert.deepEqual(explainFacts(constraint), ['<= 500', 'Activity range: 0 .. +inf', 'Diagnostics: proportional-row']);
    const variable = parseExplainResult({ kind: 'variable', name: 'x', bounds: { lower: 0, upper: null, variableKind: 'integer' } }) as ExplainResult;
    assert.deepEqual(explainFacts(variable), ['Bounds: 0 .. +inf (integer)']);
  });
});

describe('describeRequestError', () => {
  const failed = (kind: string, extra: Record<string, unknown> = {}): object => ({ code: -32803, message: 'failed', data: { kind, message: 'detail', ...extra } });

  it('recognises cancellation', () => {
    assert.equal(isCancellation({ code: REQUEST_CANCELLED }), true);
    assert.equal(isCancellation({ code: -32803 }), false);
    assert.equal(isCancellation(new Error('x')), false);
    assert.equal(isCancellation(null), false);
  });

  it('explains each failure kind', () => {
    assert.match(describeRequestError(failed('parseError', { side: 'against' })), /comparison model.*detail/);
    assert.match(describeRequestError(failed('parseError', { side: 'current' })), /this model/);
    assert.match(describeRequestError(failed('unsupportedModel')), /does not support/);
    assert.match(describeRequestError(failed('solverUnavailable', { reason: 'lp-highs not found' })), /lp-highs not found.*lp\.solver\.path/);
    assert.match(describeRequestError(failed('solverFailed')), /solver failed: detail/);
    assert.match(describeRequestError(failed('tooLarge')), /too large/);
  });

  it('handles invalid params, plain errors and junk', () => {
    assert.match(describeRequestError({ code: INVALID_PARAMS, message: 'bad limit' }), /rejected.*bad limit/);
    assert.equal(describeRequestError({ message: 'boom' }), 'boom');
    assert.equal(describeRequestError('text'), 'text');
    assert.equal(describeRequestError(undefined), 'The request failed.');
    assert.equal(describeRequestError({}), 'The request failed.');
  });

  it('strips control and bidi characters that a server may echo from model text', () => {
    const text = describeRequestError(failed('parseError', { message: 'bad ‮ name\u0000' }));
    assert.ok(!/[‮\u0000]/.test(text));
  });
});
