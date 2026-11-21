import * as assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { LIMITS, asArray, asCount, asNumber, asNumberArray, asRange, cleanName, cleanText, escapeHtml } from '../pure/sanitise';

describe('escapeHtml', () => {
  it('escapes every character that can break out of text or a quoted attribute', () => {
    assert.equal(escapeHtml(`<img src=x onerror="a('b')">&\``), '&lt;img src=x onerror=&quot;a(&#39;b&#39;)&quot;&gt;&amp;&#96;');
  });

  it('leaves plain names alone', () => {
    assert.equal(escapeHtml('capacity_42[3,4]'), 'capacity_42[3,4]');
  });
});

describe('cleanText', () => {
  it('strips control characters but keeps tabs', () => {
    assert.equal(cleanText('a\u0000b\u0007c\td\u007fe\u009ff', 100), 'abc\tdef');
  });

  it('strips bidirectional overrides and zero-width characters (Trojan Source)', () => {
    assert.equal(cleanText('x‮y⁦z​w﻿', 100), 'xyzw');
  });

  it('truncates with an ellipsis at the cap', () => {
    const cut = cleanText('a'.repeat(1000), 10);
    assert.equal(cut.length, 10);
    assert.ok(cut.endsWith('…'));
  });

  it('turns non-strings into the empty string', () => {
    for (const value of [undefined, null, 3, {}, [], true]) {
      assert.equal(cleanName(value), '');
    }
  });

  it('caps names at LIMITS.name', () => {
    assert.equal(cleanName('n'.repeat(LIMITS.name + 50)).length, LIMITS.name);
  });
});

describe('number and array readers', () => {
  it('accepts only finite numbers', () => {
    assert.equal(asNumber(1.5), 1.5);
    for (const value of [NaN, Infinity, '1', null, undefined]) {
      assert.equal(asNumber(value), undefined);
    }
  });

  it('accepts only non-negative integers as counts', () => {
    assert.equal(asCount(3), 3);
    for (const value of [-1, 1.5, '2', NaN]) {
      assert.equal(asCount(value), undefined);
    }
  });

  it('caps arrays and rejects number arrays with a bad entry', () => {
    assert.equal(asArray([1, 2, 3, 4], 2).length, 2);
    assert.deepEqual(asArray('nope', 5), []);
    assert.deepEqual(asNumberArray([1, 2, 'x'], 10), []);
    assert.deepEqual(asNumberArray([1, 2, 3], 2), [1, 2]);
  });
});

describe('asRange', () => {
  const ok = { start: { line: 1, character: 2 }, end: { line: 1, character: 5 } };

  it('accepts a well-formed range', () => {
    assert.deepEqual(asRange(ok), ok);
  });

  it('rejects null, missing, negative and reversed ranges', () => {
    assert.equal(asRange(null), undefined);
    assert.equal(asRange({ start: { line: 0, character: 0 } }), undefined);
    assert.equal(asRange({ start: { line: -1, character: 0 }, end: { line: 0, character: 0 } }), undefined);
    assert.equal(asRange({ start: { line: 2, character: 0 }, end: { line: 1, character: 9 } }), undefined);
    assert.equal(asRange({ start: { line: 1, character: 5 }, end: { line: 1, character: 2 } }), undefined);
  });
});
