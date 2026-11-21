import { MatrixDensityResult, MatrixEntriesResult, MatrixResult } from '../pure/protocol';
import {
  DETAIL_WINDOW,
  binColour,
  buildEntryLookup,
  densityScaleMax,
  describeCell,
  entryColour,
  fitCell,
  hitCell,
  pageOffset,
  windowForBin,
} from '../pure/matrixView';
import { byId, onHostMessage, post, themeColour } from './common';

const canvas = byId<HTMLCanvasElement>('canvas');
const tip = byId<HTMLDivElement>('tip');
const info = byId<HTMLSpanElement>('info');
const banner = byId<HTMLDivElement>('banner');
const stage = byId<HTMLDivElement>('stage');

let result: MatrixResult | undefined;
let lookup = new Map<number, number>();
let cell = 8;

function resizeCanvas(): CanvasRenderingContext2D | null {
  const ratio = window.devicePixelRatio || 1;
  canvas.width = Math.max(1, Math.floor(stage.clientWidth * ratio));
  canvas.height = Math.max(1, Math.floor(stage.clientHeight * ratio));
  const context = canvas.getContext('2d');
  context?.setTransform(ratio, 0, 0, ratio, 0, 0);
  return context;
}

function draw(): void {
  const context = resizeCanvas();
  if (context === null || result === undefined) {
    return;
  }
  context.clearRect(0, 0, stage.clientWidth, stage.clientHeight);
  const grid = themeColour('--vscode-panel-border', '#888');
  if (result.mode === 'entries') {
    drawEntries(context, result, grid);
  } else {
    drawDensity(context, result, grid);
  }
}

function drawEntries(context: CanvasRenderingContext2D, r: MatrixEntriesResult, grid: string): void {
  const rows = r.rows.length;
  const cols = r.cols.length;
  cell = fitCell(cols, rows, stage.clientWidth, stage.clientHeight);
  context.strokeStyle = grid;
  context.globalAlpha = 0.5;
  context.strokeRect(0.5, 0.5, cols * cell, rows * cell);
  context.globalAlpha = 1;
  const pad = cell >= 6 ? 1 : 0;
  for (let i = 0; i < r.entries.row.length; i += 1) {
    context.fillStyle = entryColour(r.entries.value[i], r.scale.minAbs, r.scale.maxAbs);
    context.fillRect(r.entries.col[i] * cell, r.entries.row[i] * cell, cell - pad, cell - pad);
  }
  info.textContent =
    `rows ${r.rowOffset + 1}–${r.rowOffset + rows} of ${r.shape.rows}, columns ${r.colOffset + 1}–${r.colOffset + cols} of ${r.shape.cols}` +
    ` · ${r.shape.nonzeros} non-zeros`;
}

function drawDensity(context: CanvasRenderingContext2D, r: MatrixDensityResult, grid: string): void {
  cell = fitCell(r.bins.cols, r.bins.rows, stage.clientWidth, stage.clientHeight, 1, 16);
  const scaleMax = densityScaleMax(r);
  context.strokeStyle = grid;
  context.globalAlpha = 0.5;
  context.strokeRect(0.5, 0.5, r.bins.cols * cell, r.bins.rows * cell);
  context.globalAlpha = 1;
  for (let row = 0; row < r.bins.rows; row += 1) {
    for (let col = 0; col < r.bins.cols; col += 1) {
      const at = row * r.bins.cols + col;
      const colour = binColour(r.count[at], r.maxAbs[at], r.sign[at], scaleMax);
      if (colour !== undefined) {
        context.fillStyle = colour;
        context.fillRect(col * cell, row * cell, cell, cell);
      }
    }
  }
  info.textContent = `overview ${r.shape.rows} × ${r.shape.cols}, ${r.shape.nonzeros} non-zeros · click a block to zoom`;
}

function pointer(event: MouseEvent): { x: number; y: number } {
  const box = canvas.getBoundingClientRect();
  return { x: event.clientX - box.left, y: event.clientY - box.top };
}

canvas.addEventListener('mousemove', (event) => {
  if (result === undefined) {
    return;
  }
  const { x, y } = pointer(event);
  let text = '';
  if (result.mode === 'entries') {
    const hit = hitCell(x, y, cell, result.rows.length, result.cols.length);
    text = hit === undefined ? '' : describeCell(result, lookup, hit);
  } else {
    const hit = hitCell(x, y, cell, result.bins.rows, result.bins.cols);
    if (hit !== undefined) {
      const at = hit.row * result.bins.cols + hit.col;
      text = `rows ${result.rowEdges[hit.row] + 1}+, columns ${result.colEdges[hit.col] + 1}+: ${result.count[at]} non-zeros`;
    }
  }
  tip.hidden = text.length === 0;
  tip.textContent = text;
  tip.style.left = `${Math.min(x + 12, stage.clientWidth - 40)}px`;
  tip.style.top = `${y + 12}px`;
});
canvas.addEventListener('mouseleave', () => {
  tip.hidden = true;
});

canvas.addEventListener('click', (event) => {
  if (result === undefined) {
    return;
  }
  const { x, y } = pointer(event);
  if (result.mode === 'density') {
    const hit = hitCell(x, y, cell, result.bins.rows, result.bins.cols);
    if (hit !== undefined) {
      post({ type: 'window', ...windowForBin(result, hit.row, hit.col) });
    }
    return;
  }
  const hit = hitCell(x, y, cell, result.rows.length, result.cols.length);
  if (hit === undefined) {
    return;
  }
  // Click opens the row's source; Shift-click opens the column's variable.
  if (event.shiftKey) {
    post({ type: 'reveal', kind: 'variable', name: result.cols[hit.col].name });
  } else {
    const row = result.rows[hit.row];
    post({ type: 'reveal', kind: row.kind === 'objective' ? 'objective' : 'constraint', name: row.name });
  }
});

function move(rowDelta: number, colDelta: number): void {
  if (result === undefined || result.mode !== 'entries') {
    return;
  }
  post({
    type: 'window',
    rowOffset: pageOffset(result.rowOffset, rowDelta, DETAIL_WINDOW, result.shape.rows),
    colOffset: pageOffset(result.colOffset, colDelta, DETAIL_WINDOW, result.shape.cols),
  });
}

byId('up').addEventListener('click', () => move(-1, 0));
byId('down').addEventListener('click', () => move(1, 0));
byId('left').addEventListener('click', () => move(0, -1));
byId('right').addEventListener('click', () => move(0, 1));
byId('overview').addEventListener('click', () => post({ type: 'mode', mode: 'density' }));
byId('detail').addEventListener('click', () => post({ type: 'mode', mode: 'entries' }));
byId('refresh').addEventListener('click', () => post({ type: 'refresh' }));
window.addEventListener('resize', draw);

onHostMessage({
  data: (message) => {
    result = message.result as MatrixResult;
    lookup = result.mode === 'entries' ? buildEntryLookup(result) : new Map();
    banner.hidden = message.stale !== true && !(result.mode === 'entries' && result.truncated);
    banner.textContent = message.stale === true ? 'The document changed since this was computed. Refresh to update.' : 'Showing a truncated set of entries.';
    canvas.setAttribute('aria-label', `Coefficient matrix, ${result.shape.rows} rows by ${result.shape.cols} columns`);
    draw();
  },
  error: (text) => {
    result = undefined;
    banner.hidden = false;
    banner.textContent = text;
    info.textContent = '';
    canvas.getContext('2d')?.clearRect(0, 0, canvas.width, canvas.height);
  },
});

post({ type: 'ready' });
