import * as assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { parseWebviewMessage } from '../pure/messages';
import { LIMITS } from '../pure/sanitise';

describe('parseWebviewMessage', () => {
  it('accepts each well-formed message', () => {
    assert.deepEqual(parseWebviewMessage({ type: 'ready' }), { type: 'ready' });
    assert.deepEqual(parseWebviewMessage({ type: 'refresh', extra: 1 }), { type: 'refresh' });
    assert.deepEqual(parseWebviewMessage({ type: 'reveal', kind: 'constraint', name: 'c1' }), { type: 'reveal', kind: 'constraint', name: 'c1' });
    assert.deepEqual(parseWebviewMessage({ type: 'explain', kind: 'variable', name: 'x' }), { type: 'explain', kind: 'variable', name: 'x' });
    assert.deepEqual(parseWebviewMessage({ type: 'revealItem', group: 'constraints.changed', index: 4 }), { type: 'revealItem', group: 'constraints.changed', index: 4 });
    assert.deepEqual(parseWebviewMessage({ type: 'window', rowOffset: 200, colOffset: 0 }), { type: 'window', rowOffset: 200, colOffset: 0 });
    assert.deepEqual(parseWebviewMessage({ type: 'mode', mode: 'density' }), { type: 'mode', mode: 'density' });
    assert.deepEqual(parseWebviewMessage({ type: 'focus', kind: 'variable', name: 'x', depth: 2 }), { type: 'focus', kind: 'variable', name: 'x', depth: 2 });
    assert.deepEqual(parseWebviewMessage({ type: 'clearFocus' }), { type: 'clearFocus' });
  });

  it('rejects non-objects, unknown types and missing fields', () => {
    for (const bad of [null, 1, 'ready', [], {}, { type: 3 }, { type: 'exec', command: 'rm' }, { type: 'reveal' }, { type: 'reveal', kind: 'constraint' }]) {
      assert.equal(parseWebviewMessage(bad), undefined);
    }
  });

  it('rejects bad entity kinds, empty and over-long names', () => {
    assert.equal(parseWebviewMessage({ type: 'reveal', kind: 'sos', name: 'a' }), undefined);
    assert.equal(parseWebviewMessage({ type: 'reveal', kind: 'variable', name: '' }), undefined);
    assert.equal(parseWebviewMessage({ type: 'reveal', kind: 'variable', name: 'n'.repeat(LIMITS.name + 1) }), undefined);
    assert.equal(parseWebviewMessage({ type: 'reveal', kind: 'variable', name: 7 }), undefined);
  });

  it('rejects malformed groups and out-of-range indices', () => {
    for (const group of ['', 'a b', '../x', 'a.b.c.d', 'x;y', 7]) {
      assert.equal(parseWebviewMessage({ type: 'revealItem', group, index: 0 }), undefined);
    }
    for (const index of [-1, 1.5, '1', 1e9]) {
      assert.equal(parseWebviewMessage({ type: 'revealItem', group: 'variables', index }), undefined);
    }
  });

  it('rejects bad windows, modes and depths', () => {
    assert.equal(parseWebviewMessage({ type: 'window', rowOffset: -1, colOffset: 0 }), undefined);
    assert.equal(parseWebviewMessage({ type: 'window', rowOffset: 0.5, colOffset: 0 }), undefined);
    assert.equal(parseWebviewMessage({ type: 'mode', mode: 'wat' }), undefined);
    assert.equal(parseWebviewMessage({ type: 'focus', kind: 'variable', name: 'x', depth: 9 }), undefined);
  });
});
