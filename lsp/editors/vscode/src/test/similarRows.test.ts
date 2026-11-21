import * as assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { LspRange } from '../pure/sanitise';
import { SimilarDiagnostic, SimilarFinding, findingsFor, rowLabel, similarFindings, similarPicks } from '../pure/similarRows';

const at = (line: number, from = 0, to = 2): LspRange => ({ start: { line, character: from }, end: { line, character: to } });
const diag = (code: string | undefined, extra: Partial<SimilarDiagnostic> = {}): SimilarDiagnostic => ({
  source: 'lp-lsp',
  code,
  message: `${code ?? 'none'} message`,
  range: at(5),
  related: [at(2)],
  ...extra,
});

describe('similarFindings', () => {
  it('keeps only lp-lsp duplicate and proportional rows that name a partner', () => {
    const { findings, notes } = similarFindings([
      diag('duplicate-row'),
      diag('proportional-row', { range: at(7) }),
      diag('dominated-row'),
      diag('duplicate-row', { source: 'other' }),
      diag('duplicate-row', { related: [] }),
      diag(undefined),
    ]);
    assert.deepEqual(findings.map((f) => [f.code, f.row.start.line, f.partner.start.line]), [
      ['duplicate-row', 5, 2],
      ['proportional-row', 7, 2],
    ]);
    assert.deepEqual(notes, []);
  });

  it('turns a truncation note about similar rows into a note', () => {
    const truncated = (code: string): SimilarDiagnostic =>
      diag('diagnostics-truncated', { message: `3 further '${code}' diagnostics were dropped after the first 100`, related: [] });
    const { findings, notes } = similarFindings([truncated('proportional-row'), truncated('large-coefficient'), truncated('duplicate-row')]);
    assert.deepEqual(findings, []);
    assert.equal(notes.length, 2);
    assert.match(notes[0], /proportional-row/);
  });
});

describe('findingsFor', () => {
  const finding = (code: 'duplicate-row', row: LspRange, partner: LspRange): SimilarFinding => ({ code, row, partner, message: 'm' });
  const all = [finding('duplicate-row', at(5), at(2)), finding('duplicate-row', at(9), at(8))];

  it('matches a row or partner starting inside the focus, ends inclusive', () => {
    assert.equal(findingsFor(all, at(2, 0, 4)).length, 1);
    assert.equal(findingsFor(all, at(5, 2, 4)).length, 0, 'starts before the focus');
    assert.equal(findingsFor(all, at(5, 0, 0)).length, 1, 'inclusive end');
    assert.equal(findingsFor(all, { start: at(2).start, end: at(9).start }).length, 2);
    assert.equal(findingsFor(all, at(20)).length, 0);
  });
});

describe('similarPicks', () => {
  const names: Record<number, string> = { 2: 'c1', 5: 'c2', 7: 'c3' };
  const nameAt = (range: LspRange): string => names[range.start.line];
  const findings: SimilarFinding[] = [
    { code: 'proportional-row', row: at(7), partner: at(2), message: 'first line\nsecond' },
    { code: 'duplicate-row', row: at(5), partner: at(2), message: 'dup' },
    { code: 'duplicate-row', row: at(5), partner: at(2), message: 'dup again' },
  ];

  it('targets the row without a focus, sorted by position, one pick per pair', () => {
    const picks = similarPicks(findings, nameAt);
    assert.deepEqual(picks.map((p) => [p.label, p.description, p.target.start.line]), [
      ['c2', 'duplicate of c1', 5],
      ['c3', 'proportional to c1', 7],
    ]);
    assert.equal(picks[1].detail, 'first line');
  });

  it('targets the other side of the focus', () => {
    const picks = similarPicks(findingsFor(findings, at(2)), nameAt, at(2));
    assert.deepEqual(picks.map((p) => [p.label, p.description]), [
      ['c2', 'duplicate of c1'],
      ['c3', 'proportional to c1'],
    ]);
    const inverse = similarPicks(findingsFor(findings, at(5)), nameAt, at(5));
    assert.deepEqual(inverse.map((p) => [p.label, p.description, p.target.start.line]), [['c1', 'duplicate of c2', 2]]);
  });
});

describe('rowLabel', () => {
  it('collapses whitespace, caps at 80, strips bidi and escapes codicons', () => {
    assert.equal(rowLabel('c1:  x\n  + y\t>= 1'), 'c1: x + y >= 1');
    assert.equal(rowLabel('a'.repeat(200)).length, 80);
    assert.equal(rowLabel('ab‮cd'), 'abcd');
    assert.equal(rowLabel('$(alert) x'), '\\$(alert) x');
  });
});
