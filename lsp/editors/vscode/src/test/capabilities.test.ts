import * as assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { LP_REQUESTS, parseLpCapabilities, supportsContextKey } from '../pure/capabilities';

describe('parseLpCapabilities', () => {
  it('reads the advertised requests', () => {
    const advertised = parseLpCapabilities({
      lp: { protocol: 1, requests: ['lp/model', 'lp/solve'], notifications: [], solver: true },
    });
    assert.deepEqual([...advertised].sort(), ['lp/model', 'lp/solve']);
  });

  it('returns the empty set when experimental is missing', () => {
    assert.equal(parseLpCapabilities(undefined).size, 0);
    assert.equal(parseLpCapabilities(null).size, 0);
    assert.equal(parseLpCapabilities({}).size, 0);
  });

  it('returns the empty set for garbled input', () => {
    assert.equal(parseLpCapabilities('lp/model').size, 0);
    assert.equal(parseLpCapabilities([{ lp: { requests: ['lp/model'] } }]).size, 0);
    assert.equal(parseLpCapabilities({ lp: 'yes' }).size, 0);
    assert.equal(parseLpCapabilities({ lp: { requests: 'lp/model' } }).size, 0);
    assert.equal(parseLpCapabilities({ lp: { requests: null } }).size, 0);
  });

  it('ignores unknown and non-string request names', () => {
    const advertised = parseLpCapabilities({ lp: { requests: [1, null, 'lp/bogus', 'lp/graph'] } });
    assert.deepEqual([...advertised], ['lp/graph']);
  });
});

describe('supportsContextKey', () => {
  it('maps every request to a distinct lp.supports key', () => {
    const keys = LP_REQUESTS.map(supportsContextKey);
    assert.equal(new Set(keys).size, LP_REQUESTS.length);
    assert.equal(supportsContextKey('lp/model'), 'lp.supports.model');
    assert.equal(supportsContextKey('lp/solverInfo'), 'lp.supports.solverInfo');
  });
});
