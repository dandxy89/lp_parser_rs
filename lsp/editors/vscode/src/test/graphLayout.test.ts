import * as assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { GraphNode, GraphResult } from '../pure/protocol';
import {
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

function node(index: number, kind: GraphNode['kind'], name: string, degree: number, component: number): GraphNode {
  return { id: index, kind, name, degree, component };
}

// c1-x, c1-y, c2-y   |   c3-z (second component)   |   w isolated (not a node here)
const graph: GraphResult = {
  version: 1,
  nodes: [
    node(0, 'constraint', 'c1', 2, 0),
    node(1, 'constraint', 'c2', 1, 0),
    node(2, 'constraint', 'c3', 1, 1),
    node(3, 'variable', 'x', 1, 0),
    node(4, 'variable', 'Y_var', 2, 0),
    node(5, 'variable', 'z', 1, 1),
  ],
  edges: { source: [0, 0, 1, 2], target: [3, 4, 4, 5], coefficient: [1, 1, 1, 1] },
  components: { count: 2, sizes: [5, 2] },
  isolated: [],
  hubs: [],
  truncated: false,
};

describe('adjacency', () => {
  const adjacency = buildAdjacency(graph);

  it('lists neighbours from both ends', () => {
    assert.deepEqual([...neighbours(adjacency, 0)].sort(), [3, 4]);
    assert.deepEqual([...neighbours(adjacency, 4)].sort(), [0, 1]);
    assert.deepEqual([...neighbours(adjacency, 5)], [2]);
  });

  it('handles an empty graph', () => {
    const empty: GraphResult = { ...graph, nodes: [], edges: { source: [], target: [], coefficient: [] } };
    assert.equal(buildAdjacency(empty).list.length, 0);
  });

  it('highlights a node and its neighbours only', () => {
    assert.deepEqual([...highlightSet(adjacency, 4)].sort(), [0, 1, 4]);
  });
});

describe('layoutBipartite', () => {
  const adjacency = buildAdjacency(graph);
  const layout = layoutBipartite(graph, adjacency);

  it('puts constraints left and variables right, each node in exactly one slot', () => {
    assert.equal(layout.order[0].length, 3);
    assert.equal(layout.order[1].length, 3);
    for (let i = 0; i < graph.nodes.length; i += 1) {
      const column = graph.nodes[i].kind === 'variable' ? 1 : 0;
      assert.equal(layout.column[i], column);
      assert.equal(layout.order[column][layout.slot[i]], i);
    }
  });

  it('keeps each component contiguous, largest component first', () => {
    for (const column of [0, 1] as const) {
      const components = [...layout.order[column]].map((n) => graph.nodes[n].component);
      assert.deepEqual(components, [...components].sort((a, b) => a - b));
    }
  });

  it('is deterministic', () => {
    const again = layoutBipartite(graph, adjacency);
    assert.deepEqual([...again.order[0]], [...layout.order[0]]);
    assert.deepEqual([...again.order[1]], [...layout.order[1]]);
  });

  it('places neighbours near each other: a chain lays out without crossings', () => {
    // r0-v1, r1-v0 should become r0 above r1 only if v1 above v0 (or vice versa): the sweep removes the crossing.
    const chain: GraphResult = {
      ...graph,
      nodes: [node(0, 'constraint', 'r0', 1, 0), node(1, 'constraint', 'r1', 1, 0), node(2, 'variable', 'v0', 1, 0), node(3, 'variable', 'v1', 1, 0)],
      edges: { source: [0, 1], target: [3, 2], coefficient: [1, 1] },
    };
    const l = layoutBipartite(chain, buildAdjacency(chain));
    const crossing = (l.slot[0] - l.slot[1]) * (l.slot[3] - l.slot[2]) < 0;
    assert.equal(crossing, false);
  });

  it('copes with an empty graph', () => {
    const empty: GraphResult = { ...graph, nodes: [], edges: { source: [], target: [], coefficient: [] } };
    const l = layoutBipartite(empty, buildAdjacency(empty));
    assert.equal(l.order[0].length + l.order[1].length, 0);
  });
});

describe('search and components', () => {
  it('matches names case-insensitively and caps hits', () => {
    assert.deepEqual(searchNodes(graph, 'y_V'), [4]);
    assert.deepEqual(searchNodes(graph, 'c'), [0, 1, 2]);
    assert.deepEqual(searchNodes(graph, 'c', 2), [0, 1]);
    assert.deepEqual(searchNodes(graph, '   '), []);
    assert.deepEqual(searchNodes(graph, 'nothing'), []);
  });

  it('masks nodes by component', () => {
    assert.deepEqual([...componentMask(graph, undefined)], [1, 1, 1, 1, 1, 1]);
    assert.deepEqual([...componentMask(graph, 1)], [0, 0, 1, 0, 0, 1]);
  });
});

describe('viewport', () => {
  it('limits drawing to the visible slots with one row of margin', () => {
    assert.deepEqual(visibleSlots(0, 100, 20, 1000), { first: 0, end: 6 });
    assert.deepEqual(visibleSlots(400, 100, 20, 1000), { first: 19, end: 26 });
    assert.deepEqual(visibleSlots(0, 100, 20, 3), { first: 0, end: 3 });
    assert.deepEqual(visibleSlots(0, 100, 0, 10), { first: 0, end: 0 });
  });

  it('never returns an inverted range when scrolled past the end', () => {
    const { first, end } = visibleSlots(10_000, 100, 20, 5);
    assert.ok(end >= first);
    assert.ok(end <= 5);
  });

  it('centres a slot and clamps at both ends', () => {
    assert.equal(scrollToSlot(0, 100, 20, 1000), 0);
    assert.equal(scrollToSlot(500, 100, 20, 1000), 500 * 20 - 50 + 10);
    assert.equal(scrollToSlot(999, 100, 20, 1000), 1000 * 20 - 100);
    assert.equal(scrollToSlot(3, 100, 20, 5), 0);
  });

  it('hit-tests pills in both columns and misses the gap', () => {
    const layout = layoutBipartite(graph, buildAdjacency(graph));
    const geometry = { rowHeight: 20, width: 100, leftX: 10, rightX: 300 };
    const leftTop = layout.order[0][0];
    const rightSecond = layout.order[1][1];
    assert.equal(hitNode(layout, 20, 5, 0, geometry), leftTop);
    assert.equal(hitNode(layout, 310, 25, 0, geometry), rightSecond);
    assert.equal(hitNode(layout, 200, 5, 0, geometry), undefined, 'gap between columns');
    assert.equal(hitNode(layout, 20, 20 * 3 + 1, 0, geometry), undefined, 'below the last pill');
    assert.equal(hitNode(layout, 20, 5, 20, geometry), layout.order[0][1], 'scrolled by one row');
    assert.equal(hitNode(layout, 20, -5, 0, geometry), undefined);
  });
});
