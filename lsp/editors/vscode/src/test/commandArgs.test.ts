import * as assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { asDocumentUri, asPosition } from '../pure/commandArgs';

describe('asDocumentUri', () => {
  it('accepts file and untitled URIs', () => {
    assert.equal(asDocumentUri('file:///a.lp'), 'file:///a.lp');
    assert.equal(asDocumentUri('untitled:Untitled-1'), 'untitled:Untitled-1');
  });

  it('rejects everything else', () => {
    const uriObject = { scheme: 'file', path: '/a.lp', fsPath: '/a.lp' };
    for (const value of [undefined, null, 5, uriObject, ['file:///a.lp'], 'https://example.com/a.lp', 'command:lp.solve', '', 'file:' + 'x'.repeat(4096)]) {
      assert.equal(asDocumentUri(value), undefined, String(JSON.stringify(value)));
    }
  });
});

describe('asPosition', () => {
  it('accepts non-negative integers and ignores extra keys', () => {
    assert.deepEqual(asPosition({ line: 0, character: 0 }), { line: 0, character: 0 });
    assert.deepEqual(asPosition({ line: 3, character: 1, extra: true }), { line: 3, character: 1 });
  });

  it('rejects negatives, fractions, NaN, strings and missing keys', () => {
    for (const value of [
      { line: -1, character: 0 },
      { line: 0, character: -1 },
      { line: 1.5, character: 0 },
      { line: 0, character: Number.NaN },
      { line: '1', character: 0 },
      { line: 1 },
      { character: 1 },
      null,
      '1:1',
    ]) {
      assert.equal(asPosition(value), undefined, String(JSON.stringify(value)));
    }
  });
});
