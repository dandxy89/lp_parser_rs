import { GraphResult } from '../pure/protocol';
import {
  Adjacency,
  Layout,
  buildAdjacency,
  componentMask,
  highlightSet,
  hitNode,
  layoutBipartite,
  neighbours,
  scrollToSlot,
  searchNodes,
  visibleSlots,
} from '../pure/graphLayout';
import { byId, clear, h, onHostMessage, post, themeColour } from './common';

const ROW = 20;
const PILL_HEIGHT = 16;

const canvas = byId<HTMLCanvasElement>('canvas');
const stage = byId<HTMLDivElement>('stage');
const info = byId<HTMLSpanElement>('info');
const banner = byId<HTMLDivElement>('banner');
const search = byId<HTMLInputElement>('search');
const componentSelect = byId<HTMLSelectElement>('component');
const depthSelect = byId<HTMLSelectElement>('depth');

let graph: GraphResult | undefined;
let adjacency: Adjacency | undefined;
let layout: Layout | undefined;
let scrollY = 0;
let selected: number | undefined;
let highlight: Set<number> | undefined;
let hits: number[] = [];
let hitCursor = 0;
let mask: Uint8Array = new Uint8Array(0);

function geometry(): { rowHeight: number; width: number; leftX: number; rightX: number } {
  const width = Math.max(80, Math.min(300, Math.floor(stage.clientWidth / 2) - 70));
  return { rowHeight: ROW, width, leftX: 12, rightX: Math.max(12 + width + 40, stage.clientWidth - 12 - width) };
}

function maxScroll(): number {
  if (layout === undefined) {
    return 0;
  }
  return Math.max(0, Math.max(layout.order[0].length, layout.order[1].length) * ROW - stage.clientHeight);
}

function draw(): void {
  const ratio = window.devicePixelRatio || 1;
  canvas.width = Math.max(1, Math.floor(stage.clientWidth * ratio));
  canvas.height = Math.max(1, Math.floor(stage.clientHeight * ratio));
  const context = canvas.getContext('2d');
  if (context === null || graph === undefined || layout === undefined || adjacency === undefined) {
    return;
  }
  context.setTransform(ratio, 0, 0, ratio, 0, 0);
  scrollY = Math.min(scrollY, maxScroll());
  const g = geometry();
  const colours = {
    text: themeColour('--vscode-foreground', '#ccc'),
    edge: themeColour('--vscode-editorLineNumber-foreground', '#888'),
    row: themeColour('--vscode-button-secondaryBackground', '#444'),
    variable: themeColour('--vscode-badge-background', '#555'),
    accent: themeColour('--vscode-focusBorder', '#09f'),
    match: themeColour('--vscode-editorWarning-foreground', '#ca5'),
  };
  context.font = `12px ${themeColour('--vscode-font-family', 'sans-serif')}`;
  context.textBaseline = 'middle';

  const left = layout.order[0];
  const right = layout.order[1];
  const visL = visibleSlots(scrollY, stage.clientHeight, ROW, left.length);
  const visR = visibleSlots(scrollY, stage.clientHeight, ROW, right.length);
  const y = (node: number): number => layout!.slot[node] * ROW - scrollY + ROW / 2;
  const isShown = (node: number): boolean => mask[node] === 1;
  const isLit = (node: number): boolean => highlight === undefined || highlight.has(node);

  // Edges first, from the visible left nodes, then from visible right nodes whose partner is off screen.
  const drawEdgesFromLeft = (from: number): void => {
    for (const to of neighbours(adjacency!, from)) {
      if (!isShown(from) || !isShown(to)) {
        continue;
      }
      const lit = highlight === undefined || (highlight.has(from) && highlight.has(to));
      context.globalAlpha = highlight === undefined ? 0.35 : lit ? 0.9 : 0.05;
      context.strokeStyle = lit && highlight !== undefined ? colours.accent : colours.edge;
      context.beginPath();
      context.moveTo(g.leftX + g.width, y(from));
      context.lineTo(g.rightX, y(to));
      context.stroke();
    }
  };
  for (let slot = visL.first; slot < visL.end; slot += 1) {
    drawEdgesFromLeft(left[slot]);
  }
  for (let slot = visR.first; slot < visR.end; slot += 1) {
    const node = right[slot];
    for (const partner of neighbours(adjacency, node)) {
      const partnerSlot = layout.slot[partner];
      if (partnerSlot < visL.first || partnerSlot >= visL.end) {
        if (!isShown(node) || !isShown(partner)) {
          continue;
        }
        const lit = highlight === undefined || (highlight.has(node) && highlight.has(partner));
        context.globalAlpha = highlight === undefined ? 0.35 : lit ? 0.9 : 0.05;
        context.strokeStyle = lit && highlight !== undefined ? colours.accent : colours.edge;
        context.beginPath();
        context.moveTo(g.leftX + g.width, y(partner));
        context.lineTo(g.rightX, y(node));
        context.stroke();
      }
    }
  }

  const maxChars = Math.floor(g.width / 7);
  const pill = (node: number, x: number, fill: string): void => {
    const shown = isShown(node);
    context.globalAlpha = !shown ? 0.1 : isLit(node) ? 1 : 0.25;
    context.fillStyle = fill;
    const top = y(node) - PILL_HEIGHT / 2;
    context.fillRect(x, top, g.width, PILL_HEIGHT);
    if (node === selected || hits.includes(node)) {
      context.strokeStyle = node === selected ? colours.accent : colours.match;
      context.lineWidth = 2;
      context.strokeRect(x, top, g.width, PILL_HEIGHT);
      context.lineWidth = 1;
    }
    const name = graph!.nodes[node].name;
    context.fillStyle = colours.text;
    context.fillText(name.length > maxChars ? `${name.slice(0, maxChars - 1)}…` : name, x + 4, y(node));
  };
  for (let slot = visL.first; slot < visL.end; slot += 1) {
    pill(left[slot], g.leftX, colours.row);
  }
  for (let slot = visR.first; slot < visR.end; slot += 1) {
    pill(right[slot], g.rightX, colours.variable);
  }
  context.globalAlpha = 1;
}

function setGraph(next: GraphResult): void {
  graph = next;
  adjacency = buildAdjacency(next);
  layout = layoutBipartite(next, adjacency);
  mask = componentMask(next, undefined);
  selected = undefined;
  highlight = undefined;
  hits = [];
  scrollY = 0;
  clear(componentSelect);
  componentSelect.append(h('option', { text: 'All components' }));
  componentSelect.firstElementChild?.setAttribute('value', '');
  next.components.sizes.forEach((size, index) => {
    const option = h('option', { text: `Component ${index + 1} (${size} nodes)` });
    option.setAttribute('value', String(index));
    componentSelect.append(option);
  });
  const rows = next.nodes.filter((n) => n.kind !== 'variable').length;
  info.textContent =
    `${rows} rows, ${next.nodes.length - rows} variables, ${next.edges.source.length} edges, ${next.components.count} components` +
    (next.isolated.length > 0 ? `, ${next.isolated.length} isolated variables` : '');
}

function select(node: number | undefined): void {
  selected = node;
  highlight = node === undefined || adjacency === undefined ? undefined : highlightSet(adjacency, node);
  draw();
}

function nodeAt(event: MouseEvent): number | undefined {
  if (layout === undefined) {
    return undefined;
  }
  const box = canvas.getBoundingClientRect();
  return hitNode(layout, event.clientX - box.left, event.clientY - box.top, scrollY, geometry());
}

canvas.addEventListener('click', (event) => select(nodeAt(event)));
canvas.addEventListener('dblclick', (event) => {
  const node = nodeAt(event);
  if (graph !== undefined && node !== undefined) {
    post({ type: 'reveal', kind: graph.nodes[node].kind, name: graph.nodes[node].name });
  }
});
canvas.addEventListener(
  'wheel',
  (event) => {
    event.preventDefault();
    scrollY = Math.min(maxScroll(), Math.max(0, scrollY + event.deltaY));
    draw();
  },
  { passive: false },
);
canvas.tabIndex = 0;
canvas.addEventListener('keydown', (event) => {
  const step = event.key === 'PageDown' ? stage.clientHeight : event.key === 'PageUp' ? -stage.clientHeight : event.key === 'ArrowDown' ? ROW : event.key === 'ArrowUp' ? -ROW : 0;
  if (step !== 0) {
    event.preventDefault();
    scrollY = Math.min(maxScroll(), Math.max(0, scrollY + step));
    draw();
  }
});

search.addEventListener('input', () => {
  hits = graph === undefined ? [] : searchNodes(graph, search.value);
  hitCursor = 0;
  jumpToHit();
});
search.addEventListener('keydown', (event) => {
  if (event.key === 'Enter' && hits.length > 0) {
    hitCursor = (hitCursor + (event.shiftKey ? hits.length - 1 : 1)) % hits.length;
    jumpToHit();
  }
});

function jumpToHit(): void {
  if (layout !== undefined && hits.length > 0) {
    const node = hits[hitCursor];
    const slot = layout.slot[node];
    scrollY = scrollToSlot(slot, stage.clientHeight, ROW, layout.order[layout.column[node]].length);
    info.textContent = `${hits.length} match${hits.length === 1 ? '' : 'es'} (Enter for next)`;
  }
  draw();
}

componentSelect.addEventListener('change', () => {
  if (graph !== undefined) {
    mask = componentMask(graph, componentSelect.value === '' ? undefined : Number(componentSelect.value));
    draw();
  }
});

byId('focus').addEventListener('click', () => {
  if (graph !== undefined && selected !== undefined) {
    post({ type: 'focus', kind: graph.nodes[selected].kind, name: graph.nodes[selected].name, depth: Number(depthSelect.value) });
  }
});
byId('reveal').addEventListener('click', () => {
  if (graph !== undefined && selected !== undefined) {
    post({ type: 'reveal', kind: graph.nodes[selected].kind, name: graph.nodes[selected].name });
  }
});
byId('clearFocus').addEventListener('click', () => post({ type: 'clearFocus' }));
byId('refresh').addEventListener('click', () => post({ type: 'refresh' }));
window.addEventListener('resize', draw);

onHostMessage({
  data: (message) => {
    setGraph(message.result as GraphResult);
    const stale = message.stale === true;
    const truncated = graph?.truncated === true;
    banner.hidden = !stale && !truncated;
    banner.textContent = stale
      ? 'The document changed since this was computed. Refresh to update.'
      : 'The graph was truncated to the size limits. Focus on a node to see its neighbourhood.';
    draw();
  },
  error: (text) => {
    graph = undefined;
    banner.hidden = false;
    banner.textContent = text;
    info.textContent = '';
    canvas.getContext('2d')?.clearRect(0, 0, canvas.width, canvas.height);
  },
});
post({ type: 'ready' });
