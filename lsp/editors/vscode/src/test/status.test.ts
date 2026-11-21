import * as assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { formatStatusText, formatStats, formatStatusTooltip, isValid } from '../pure/status';

describe('formatStatusText', () => {
  it('shows server states without counts', () => {
    assert.equal(formatStatusText('starting'), '$(sync~spin) LP');
    assert.equal(formatStatusText('failed'), '$(error) LP: server failed');
    assert.equal(formatStatusText('stopped'), '$(circle-slash) LP: stopped');
    assert.equal(formatStatusText('running'), 'LP');
  });

  it('shows error and warning counts for a running server', () => {
    assert.equal(formatStatusText('running', { errors: 0, warnings: 0 }), 'LP $(error) 0 $(warning) 0');
    assert.equal(formatStatusText('running', { errors: 1, warnings: 0 }), 'LP $(error) 1 $(warning) 0');
    assert.equal(formatStatusText('running', { errors: 0, warnings: 1 }), 'LP $(error) 0 $(warning) 1');
    assert.equal(formatStatusText('running', { errors: 3, warnings: 12 }), 'LP $(error) 3 $(warning) 12');
  });

  it('rejects counts that are not non-negative integers', () => {
    assert.throws(() => formatStatusText('running', { errors: -1, warnings: 0 }), RangeError);
    assert.throws(() => formatStatusText('running', { errors: 0, warnings: 1.5 }), RangeError);
    assert.throws(() => formatStatusText('running', { errors: Number.NaN, warnings: 0 }), RangeError);
  });
});

describe('model stats in the status bar', () => {
  const stats = { objectives: 1, constraints: 2, variables: 3 };

  it('appends size and a validity tick', () => {
    assert.equal(formatStatusText('running', { errors: 0, warnings: 1 }, stats), 'LP 3 vars, 2 cons $(check) 0 $(warning) 1');
    assert.equal(formatStatusText('running', { errors: 2, warnings: 0 }, stats), 'LP 3 vars, 2 cons $(error) 2 $(warning) 0');
    assert.equal(formatStatusText('running', undefined, stats), 'LP 3 vars, 2 cons');
  });

  it('mentions objectives only when there is not exactly one', () => {
    assert.equal(formatStats({ ...stats, objectives: 0 }), '3 vars, 2 cons, 0 objs');
    assert.equal(formatStats({ ...stats, objectives: 2 }), '3 vars, 2 cons, 2 objs');
  });

  it('builds a tooltip with validity', () => {
    assert.match(formatStatusTooltip('running', { errors: 0, warnings: 0 }, stats), /Model: 3 vars, 2 cons\nValid: no errors/);
    assert.match(formatStatusTooltip('running', { errors: 2, warnings: 0 }), /Invalid: 2 error\(s\)/);
    assert.equal(isValid(undefined), undefined);
  });

  it('rejects bad stats', () => {
    assert.throws(() => formatStats({ ...stats, variables: -1 }), RangeError);
  });
});
