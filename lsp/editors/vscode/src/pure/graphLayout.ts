import { GraphResult } from './protocol';

/** Compressed adjacency: neighbours of node `n` are `list[start[n] .. start[n + 1]]`. */
export interface Adjacency {
  readonly start: Uint32Array;
  readonly list: Uint32Array;
}

export function buildAdjacency(graph: GraphResult): Adjacency {
  const n = graph.nodes.length;
  const degree = new Uint32Array(n + 1);
  const edges = graph.edges.source.length;
  for (let i = 0; i < edges; i += 1) {
    degree[graph.edges.source[i] + 1] += 1;
    degree[graph.edges.target[i] + 1] += 1;
  }
  for (let i = 0; i < n; i += 1) {
    degree[i + 1] += degree[i];
  }
  const fill = degree.slice(0, n);
  const list = new Uint32Array(edges * 2);
  for (let i = 0; i < edges; i += 1) {
    list[fill[graph.edges.source[i]]++] = graph.edges.target[i];
    list[fill[graph.edges.target[i]]++] = graph.edges.source[i];
  }
  return { start: degree, list };
}

export function neighbours(adjacency: Adjacency, node: number): Uint32Array {
  return adjacency.list.subarray(adjacency.start[node], adjacency.start[node + 1]);
}

export interface Layout {
  /** Column per node: 0 = rows and objectives (left), 1 = variables (right). */
  readonly column: Uint8Array;
  /** Vertical slot per node within its column. */
  readonly slot: Uint32Array;
  /** Node index per slot, per column. */
  readonly order: readonly [Uint32Array, Uint32Array];
}

/**
 * Bipartite layout: both columns grouped by component (largest first), then ordered by two barycentre
 * sweeps so connected nodes sit near each other and edges cross less. O((nodes + edges) log nodes).
 */
export function layoutBipartite(graph: GraphResult, adjacency: Adjacency, sweeps = 2): Layout {
  const n = graph.nodes.length;
  const column = new Uint8Array(n);
  const left: number[] = [];
  const right: number[] = [];
  graph.nodes.forEach((node, index) => {
    column[index] = node.kind === 'variable' ? 1 : 0;
    (node.kind === 'variable' ? right : left).push(index);
  });
  const byDegree = (a: number, b: number): number =>
    graph.nodes[a].component - graph.nodes[b].component || graph.nodes[b].degree - graph.nodes[a].degree || a - b;
  left.sort(byDegree);
  right.sort(byDegree);
  const slot = new Uint32Array(n);
  const assign = (order: number[]): void => order.forEach((node, i) => (slot[node] = i));
  assign(left);
  assign(right);

  const sweep = (order: number[]): void => {
    const key = new Float64Array(n);
    for (const node of order) {
      const around = neighbours(adjacency, node);
      let sum = 0;
      for (const other of around) {
        sum += slot[other];
      }
      key[node] = around.length > 0 ? sum / around.length : slot[node];
    }
    order.sort((a, b) => graph.nodes[a].component - graph.nodes[b].component || key[a] - key[b] || a - b);
    assign(order);
  };
  for (let i = 0; i < sweeps; i += 1) {
    sweep(right);
    sweep(left);
  }
  return { column, slot, order: [Uint32Array.from(left), Uint32Array.from(right)] };
}

/** Indices of nodes whose name contains `query` (case-insensitive), at most `max`. */
export function searchNodes(graph: GraphResult, query: string, max = 1000): number[] {
  const needle = query.trim().toLowerCase();
  if (needle.length === 0) {
    return [];
  }
  const hits: number[] = [];
  for (let i = 0; i < graph.nodes.length && hits.length < max; i += 1) {
    if (graph.nodes[i].name.toLowerCase().includes(needle)) {
      hits.push(i);
    }
  }
  return hits;
}

/** A node and its direct neighbours, for click-to-highlight. */
export function highlightSet(adjacency: Adjacency, node: number): Set<number> {
  const set = new Set<number>([node]);
  for (const other of neighbours(adjacency, node)) {
    set.add(other);
  }
  return set;
}

/** First and one-past-last slot visible for a scrolled column; slots outside are not drawn. */
export function visibleSlots(scrollY: number, viewHeight: number, rowHeight: number, count: number): { first: number; end: number } {
  if (!(rowHeight > 0)) {
    return { first: 0, end: 0 };
  }
  const first = Math.max(0, Math.floor(scrollY / rowHeight) - 1);
  const end = Math.min(count, Math.ceil((scrollY + viewHeight) / rowHeight) + 1);
  return { first: Math.min(first, count), end: Math.max(end, Math.min(first, count)) };
}

/** Scroll offset that centres `slot` in a view, clamped to the content. */
export function scrollToSlot(slot: number, viewHeight: number, rowHeight: number, count: number): number {
  const max = Math.max(0, count * rowHeight - viewHeight);
  return Math.min(max, Math.max(0, slot * rowHeight - viewHeight / 2 + rowHeight / 2));
}

/** The node at canvas point `(x, y)`, given node pill geometry; `undefined` between pills. */
export function hitNode(
  layout: Layout,
  x: number,
  y: number,
  scrollY: number,
  geometry: { rowHeight: number; width: number; leftX: number; rightX: number },
): number | undefined {
  const slotAt = Math.floor((y + scrollY) / geometry.rowHeight);
  if (slotAt < 0) {
    return undefined;
  }
  const inLeft = x >= geometry.leftX && x <= geometry.leftX + geometry.width;
  const inRight = x >= geometry.rightX && x <= geometry.rightX + geometry.width;
  if (!inLeft && !inRight) {
    return undefined;
  }
  const order = layout.order[inLeft ? 0 : 1];
  return slotAt < order.length ? order[slotAt] : undefined;
}

/** Node indices of component `component`, or all nodes when `component` is `undefined`. */
export function componentMask(graph: GraphResult, component: number | undefined): Uint8Array {
  const mask = new Uint8Array(graph.nodes.length);
  graph.nodes.forEach((node, i) => {
    mask[i] = component === undefined || node.component === component ? 1 : 0;
  });
  return mask;
}
