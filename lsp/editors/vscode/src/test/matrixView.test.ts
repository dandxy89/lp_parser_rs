import * as assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { MatrixDensityResult, MatrixEntriesResult } from '../pure/protocol';
import {
  DETAIL_WINDOW,
  binColour,
  buildEntryLookup,
  clampOffset,
  defaultMode,
  densityScaleMax,
  describeCell,
  entryColour,
  fitCell,
  hitCell,
  logPosition,
  pageOffset,
  windowForBin,
} from '../pure/matrixView';

const entries: MatrixEntriesResult = {
  mode: 'entries',
  version: 1,
  shape: { rows: 2, cols: 3, nonzeros: 3 },
  rowOffset: 0,
  colOffset: 0,
  rows: [{ name: 'obj', kind: 'objective' }, { name: 'c1', kind: 'linear' }],
  cols: [{ name: 'x', kind: 'continuous' }, { name: 'y', kind: 'continuous' }, { name: 'z', kind: 'continuous' }],
  entries: { row: [0, 1, 1], col: [0, 0, 2], value: [1, 2, -3] },
  scale: { minAbs: 1, maxAbs: 3 },
  truncated: false,
};

describe('mode and paging', () => {
  it('uses detail when the model fits one window, otherwise the overview', () => {
    assert.equal(defaultMode({ rows: 200, cols: 200 }), 'entries');
    assert.equal(defaultMode({ rows: 201, cols: 10 }), 'density');
    assert.equal(defaultMode({ rows: 10, cols: 5000 }), 'density');
  });

  it('clamps offsets so a full window stays in range', () => {
    assert.equal(clampOffset(-5, 200, 1000), 0);
    assert.equal(clampOffset(950, 200, 1000), 800);
    assert.equal(clampOffset(10, 200, 50), 0);
    assert.equal(clampOffset(12.7, 200, 1000), 12);
  });

  it('pages by whole windows', () => {
    assert.equal(pageOffset(0, 1, 200, 1000), 200);
    assert.equal(pageOffset(200, -1, 200, 1000), 0);
    assert.equal(pageOffset(0, -1, 200, 1000), 0);
    assert.equal(pageOffset(800, 1, 200, 1000), 800);
  });
});

describe('fitCell', () => {
  it('fits the window inside the box within the size limits', () => {
    assert.equal(fitCell(100, 50, 800, 400), 8);
    assert.equal(fitCell(2, 2, 800, 400), 24);
    assert.equal(fitCell(1000, 1000, 100, 100), 3);
  });

  it('survives zero sizes', () => {
    assert.equal(fitCell(0, 0, 100, 100), 3);
    assert.equal(fitCell(10, 10, 0, 0), 3);
  });
});

describe('colour scale', () => {
  it('places magnitudes on a log scale between the smallest and largest', () => {
    assert.equal(logPosition(1, 1, 1000), 0);
    assert.equal(logPosition(1000, 1, 1000), 1);
    assert.ok(Math.abs(logPosition(10, 1, 1000) - 1 / 3) < 1e-12);
    assert.equal(logPosition(-1000, 1, 1000), 1);
  });

  it('handles zero, a flat scale and values outside the scale', () => {
    assert.equal(logPosition(0, 1, 10), 0);
    assert.equal(logPosition(5, 5, 5), 1);
    assert.equal(logPosition(1e9, 1, 10), 1);
    assert.equal(logPosition(1e-9, 1, 10), 0);
  });

  it('colours positive blue and negative orange, stronger for larger values', () => {
    assert.match(entryColour(5, 1, 10), /^hsla\(212/);
    assert.match(entryColour(-5, 1, 10), /^hsla\(28/);
    const alpha = (s: string): number => Number(/,\s*([0-9.]+)\)$/.exec(s)?.[1]);
    assert.ok(alpha(entryColour(10, 1, 10)) > alpha(entryColour(1, 1, 10)));
  });

  it('leaves empty density bins uncoloured and marks mixed signs', () => {
    assert.equal(binColour(0, 0, 0, 10), undefined);
    assert.match(binColour(3, 5, 0, 10) ?? '', /^hsla\(285/);
    assert.match(binColour(3, 5, 1, 10) ?? '', /^hsla\(212/);
    assert.match(binColour(3, 5, -1, 10) ?? '', /^hsla\(28/);
  });
});

describe('hit testing', () => {
  it('maps points to window-relative cells', () => {
    assert.deepEqual(hitCell(0, 0, 10, 2, 3), { row: 0, col: 0 });
    assert.deepEqual(hitCell(25, 15, 10, 2, 3), { row: 1, col: 2 });
  });

  it('misses outside the grid and for degenerate input', () => {
    assert.equal(hitCell(30, 0, 10, 2, 3), undefined);
    assert.equal(hitCell(0, 20, 10, 2, 3), undefined);
    assert.equal(hitCell(-1, 0, 10, 2, 3), undefined);
    assert.equal(hitCell(1, 1, 0, 2, 3), undefined);
  });

  it('looks up entries and describes cells, naming empty ones', () => {
    const lookup = buildEntryLookup(entries);
    assert.equal(lookup.size, 3);
    assert.equal(describeCell(entries, lookup, { row: 1, col: 2 }), 'c1 × z: -3');
    assert.equal(describeCell(entries, lookup, { row: 0, col: 1 }), 'obj × y: 0 (no entry)');
    assert.equal(describeCell(entries, lookup, { row: 9, col: 9 }), '');
  });
});

describe('density overview', () => {
  const density: MatrixDensityResult = {
    mode: 'density',
    version: 1,
    shape: { rows: 1000, cols: 500, nonzeros: 10 },
    bins: { rows: 2, cols: 2 },
    rowEdges: [0, 500],
    colEdges: [0, 250],
    count: [1, 0, 0, 2],
    maxAbs: [5, 0, 0, 9],
    sign: [1, 0, 0, -1],
  };

  it('centres the zoom window on a bin and clamps it', () => {
    const first = windowForBin(density, 0, 0);
    assert.deepEqual(first, { rowOffset: 150, colOffset: 25 });
    const last = windowForBin(density, 1, 1);
    assert.equal(last.rowOffset, 650);
    assert.equal(last.colOffset, 275);
    assert.ok(last.rowOffset + DETAIL_WINDOW <= density.shape.rows);
  });

  it('finds the largest bin value', () => {
    assert.equal(densityScaleMax(density), 9);
  });
});
