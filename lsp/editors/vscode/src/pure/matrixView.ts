import { MatrixDensityResult, MatrixEntriesResult } from './protocol';

/** Largest window the detail view asks for (contract default). */
export const DETAIL_WINDOW = 200;

export type MatrixMode = 'entries' | 'density';

/** Detail for models that fit one window, otherwise the density overview. */
export function defaultMode(shape: { rows: number; cols: number }): MatrixMode {
  return shape.rows <= DETAIL_WINDOW && shape.cols <= DETAIL_WINDOW ? 'entries' : 'density';
}

/** Largest offset that still shows a full window (0 when everything fits). */
export function clampOffset(offset: number, size: number, total: number): number {
  const max = Math.max(0, total - size);
  return Math.min(Math.max(0, Math.floor(offset)), max);
}

/** Offset after paging by `delta` windows (negative = back). */
export function pageOffset(offset: number, delta: number, size: number, total: number): number {
  return clampOffset(offset + delta * size, size, total);
}

/** Pixel size of one cell so a `cols x rows` window fits `width x height`, within `[min, max]`. */
export function fitCell(cols: number, rows: number, width: number, height: number, min = 3, max = 24): number {
  if (cols <= 0 || rows <= 0 || width <= 0 || height <= 0) {
    return min;
  }
  return Math.max(min, Math.min(max, Math.floor(Math.min(width / cols, height / rows))));
}

/** 0..1 position of `value` on a log scale between the matrix's smallest and largest magnitude. */
export function logPosition(value: number, minAbs: number, maxAbs: number): number {
  const magnitude = Math.abs(value);
  if (!(magnitude > 0) || !(maxAbs > 0)) {
    return 0;
  }
  const lo = Math.log10(Math.max(minAbs, Number.MIN_VALUE));
  const hi = Math.log10(maxAbs);
  if (hi <= lo) {
    return 1;
  }
  return Math.min(1, Math.max(0, (Math.log10(magnitude) - lo) / (hi - lo)));
}

/** Translucent so it reads on light and dark themes: blue for positive, orange for negative. */
export function entryColour(value: number, minAbs: number, maxAbs: number): string {
  const t = logPosition(value, minAbs, maxAbs);
  const hue = value < 0 ? 28 : 212;
  return `hsla(${hue}, 80%, 52%, ${(0.3 + 0.7 * t).toFixed(3)})`;
}

/** Colour of a density bin: `sign` 1 positive, -1 negative, 0 mixed (purple); empty bins are `undefined`. */
export function binColour(count: number, maxAbs: number, sign: number, scaleMax: number): string | undefined {
  if (count <= 0) {
    return undefined;
  }
  const t = scaleMax > 0 ? Math.min(1, Math.log1p(maxAbs) / Math.log1p(scaleMax)) : 1;
  const hue = sign > 0 ? 212 : sign < 0 ? 28 : 285;
  return `hsla(${hue}, 75%, 52%, ${(0.25 + 0.75 * t).toFixed(3)})`;
}

export interface Cell {
  readonly row: number;
  readonly col: number;
}

/** The window-relative cell under a canvas point, or `undefined` outside the grid. */
export function hitCell(x: number, y: number, cell: number, rows: number, cols: number): Cell | undefined {
  if (!(cell > 0) || x < 0 || y < 0) {
    return undefined;
  }
  const col = Math.floor(x / cell);
  const row = Math.floor(y / cell);
  return row < rows && col < cols ? { row, col } : undefined;
}

/** Sparse lookup of the non-zero entries of a window: key = `row * cols + col`. */
export function buildEntryLookup(result: MatrixEntriesResult): Map<number, number> {
  const lookup = new Map<number, number>();
  const cols = result.cols.length;
  for (let i = 0; i < result.entries.row.length; i += 1) {
    lookup.set(result.entries.row[i] * cols + result.entries.col[i], result.entries.value[i]);
  }
  return lookup;
}

/** Text for a hovered cell. Names are plain text (the webview sets it with `textContent`). */
export function describeCell(result: MatrixEntriesResult, lookup: ReadonlyMap<number, number>, cell: Cell): string {
  const row = result.rows[cell.row];
  const col = result.cols[cell.col];
  if (row === undefined || col === undefined) {
    return '';
  }
  const value = lookup.get(cell.row * result.cols.length + cell.col);
  return `${row.name} × ${col.name}: ${value === undefined ? '0 (no entry)' : String(value)}`;
}

/** The window to request when the user clicks density bin `(binRow, binCol)`: centred on the bin, clamped. */
export function windowForBin(result: MatrixDensityResult, binRow: number, binCol: number): { rowOffset: number; colOffset: number } {
  const rowStart = result.rowEdges[binRow] ?? 0;
  const colStart = result.colEdges[binCol] ?? 0;
  const rowEnd = result.rowEdges[binRow + 1] ?? result.shape.rows;
  const colEnd = result.colEdges[binCol + 1] ?? result.shape.cols;
  return {
    rowOffset: clampOffset((rowStart + rowEnd) / 2 - DETAIL_WINDOW / 2, DETAIL_WINDOW, result.shape.rows),
    colOffset: clampOffset((colStart + colEnd) / 2 - DETAIL_WINDOW / 2, DETAIL_WINDOW, result.shape.cols),
  };
}

/** Largest bin maximum, for scaling density colours. */
export function densityScaleMax(result: MatrixDensityResult): number {
  let max = 0;
  for (const value of result.maxAbs) {
    if (value > max) {
      max = value;
    }
  }
  return max;
}
