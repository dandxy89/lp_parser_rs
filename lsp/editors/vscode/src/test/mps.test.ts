import * as assert from 'node:assert/strict';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { describe, it } from 'node:test';
import { parseConvertFromMpsResult } from '../pure/mps';

interface Rule {
  match?: string;
  begin?: string;
  end?: string;
  patterns?: Rule[];
}

const grammar = JSON.parse(fs.readFileSync(path.join(__dirname, '..', '..', 'syntaxes', 'mps.tmLanguage.json'), 'utf8')) as {
  repository: Record<string, Rule>;
};

function compile(source: string): RegExp {
  const insensitive = source.startsWith('(?i)');
  return new RegExp(insensitive ? source.slice(4) : source, insensitive ? 'i' : '');
}

function collect(rule: Rule, out: string[]): void {
  for (const key of ['match', 'begin', 'end'] as const) {
    const value = rule[key];
    if (value !== undefined) {
      out.push(value);
    }
  }
  rule.patterns?.forEach((child) => collect(child, out));
}

describe('parseConvertFromMpsResult', () => {
  it('accepts a valid result', () => {
    assert.deepEqual(parseConvertFromMpsResult({ lp: 'End', omitted: ['c3'] }), { lp: 'End', omitted: ['c3'] });
  });

  it('rejects a result without string lp text', () => {
    assert.equal(parseConvertFromMpsResult({ lp: 1 }), undefined);
    assert.equal(parseConvertFromMpsResult(null), undefined);
    assert.equal(parseConvertFromMpsResult('End'), undefined);
  });

  it('drops non-string omitted entries and defaults to none', () => {
    assert.deepEqual(parseConvertFromMpsResult({ lp: 'x', omitted: ['a', 1, null] }), { lp: 'x', omitted: ['a'] });
    assert.deepEqual(parseConvertFromMpsResult({ lp: 'x' }), { lp: 'x', omitted: [] });
    assert.deepEqual(parseConvertFromMpsResult({ lp: 'x', omitted: 'a' }), { lp: 'x', omitted: [] });
  });
});

describe('MPS grammar', () => {
  it('compiles every pattern', () => {
    const sources: string[] = [];
    Object.values(grammar.repository).forEach((rule) => collect(rule, sources));
    assert.ok(sources.length > 10);
    for (const source of sources) {
      assert.doesNotThrow(() => compile(source), source);
    }
  });

  it('recognises comments, section headers, rows and bounds', () => {
    assert.ok(compile(grammar.repository.comment.match ?? '').test('* note'));
    const section = compile(grammar.repository.section.match ?? '');
    assert.ok(section.test('ROWS'));
    assert.ok(!section.test(' ROWS'));
    const row = compile(grammar.repository.rows.patterns?.[1].match ?? '').exec(' L  c1');
    assert.deepEqual(row?.slice(1, 3), ['L', 'c1']);
    assert.ok(compile(grammar.repository.bounds.patterns?.[1].match ?? '').test(' UP BND x 4'));
  });
});
