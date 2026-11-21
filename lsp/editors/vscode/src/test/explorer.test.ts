import * as assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { type ExplorerNode, ModelExplorer, formatCoefficient, isSpecial } from '../pure/explorer';
import type { ConstraintItem, EntityKind, ExplainResult, ModelClient, ModelResult, VariableItem } from '../pure/modelTypes';

const r = (line: number) => ({ start: { line, character: 0 }, end: { line, character: 5 } });

const variable = (name: string, kind: string, extra: Partial<VariableItem> = {}): VariableItem => ({
  name,
  kind,
  bounds: { lower: 0, upper: null },
  usage: { objectives: 1, constraints: 2, quadratic: 0 },
  unused: false,
  free: false,
  range: r(9),
  ...extra,
});

const constraint = (name: string, extra: Partial<ConstraintItem> = {}): ConstraintItem => ({
  name,
  generated: false,
  kind: 'linear',
  class: 'normal',
  operator: '<=',
  rhs: 100,
  terms: 2,
  rangePartner: null,
  range: r(4),
  selectionRange: r(4),
  ...extra,
});

const MODEL: ModelResult = {
  uri: 'file:///m.lp',
  version: 1,
  summary: {
    name: null,
    sense: 'Minimize',
    objectives: 1,
    constraints: 4,
    variables: 4,
    nonzeros: 5,
    density: 0.5,
    variablesByKind: { continuous: 2, binary: 1, integer: 1 },
    constraintsByKind: { linear: 3 },
    diagnostics: { error: 0, warning: 0, info: 0 },
  },
  objectives: [{ name: 'obj', range: r(1), selectionRange: r(1), terms: 3, quadraticTerms: 0 }],
  constraints: [
    constraint('c1'),
    constraint('c2', { operator: '>=', rhs: 5, rangePartner: 'c2_rng' }),
    constraint('lim', { class: 'lazy' }),
    constraint('s1', { kind: 'sos1', operator: undefined, rhs: undefined }),
  ],
  variables: [variable('y', 'binary'), variable('x', 'continuous'), variable('z', 'continuous', { unused: true }), variable('n', 'integer', { free: true })],
  truncated: { objectives: false, constraints: false, variables: false },
};

class MockClient implements ModelClient {
  modelCalls = 0;
  explainCalls: { kind: EntityKind; name: string }[] = [];
  model(): Promise<ModelResult> {
    this.modelCalls += 1;
    return Promise.resolve(MODEL);
  }
  explain(uri: string, target: { kind: EntityKind; name: string }): Promise<ExplainResult> {
    this.explainCalls.push(target);
    const base = { uri, version: 1, kind: target.kind, name: target.name, range: null, markdown: '' };
    if (target.kind === 'variable') {
      return Promise.resolve({
        ...base,
        usage: [
          { entity: 'c1', kind: 'constraint', role: 'linear', coefficient: 2, range: r(4) },
          { entity: 'obj', kind: 'objective', role: 'linear', coefficient: null, range: r(1) },
        ],
      });
    }
    return Promise.resolve({
      ...base,
      terms: [
        { variable: 'x', coefficient: 2, range: r(4) },
        { variable: 'y', coefficient: -1, range: r(4) },
        { variable: 'w', coefficient: 1, range: null },
      ],
      termsTruncated: true,
    });
  }
}

async function loaded(): Promise<{ explorer: ModelExplorer; client: MockClient }> {
  const client = new MockClient();
  const explorer = new ModelExplorer(client);
  explorer.setDocument(MODEL.uri);
  return { explorer, client };
}

describe('ModelExplorer roots', () => {
  it('asks for an LP file when no document is active', async () => {
    const explorer = new ModelExplorer(new MockClient());
    const roots = await explorer.roots();
    assert.equal(roots.length, 1);
    assert.equal(roots[0].kind, 'notice');
  });

  it('shows objectives, constraints, variables and special with counts', async () => {
    const { explorer } = await loaded();
    const roots = await explorer.roots();
    assert.deepEqual(
      roots.map((n) => [n.label, n.description]),
      [
        ['Objectives', '1'],
        ['Constraints', '2'],
        ['Variables', '4'],
        ['Special', '2'],
      ],
    );
  });

  it('puts lazy rows and non-linear kinds under Special', () => {
    assert.equal(isSpecial(constraint('a')), false);
    assert.equal(isSpecial(constraint('a', { class: 'userCut' })), true);
    assert.equal(isSpecial(constraint('a', { kind: 'indicator' })), true);
  });

  it('caches the model until invalidated and reloads on document change', async () => {
    const { explorer, client } = await loaded();
    await explorer.roots();
    await explorer.roots();
    assert.equal(client.modelCalls, 1);
    explorer.invalidate();
    await explorer.roots();
    assert.equal(client.modelCalls, 2);
    explorer.setDocument('file:///other.lp');
    await explorer.roots();
    assert.equal(client.modelCalls, 3);
  });

  it('reports a request failure as an error node instead of throwing', async () => {
    const client: ModelClient = {
      model: () => Promise.reject(new Error('boom')),
      explain: () => Promise.reject(new Error('unused')),
    };
    const explorer = new ModelExplorer(client);
    explorer.setDocument('file:///m.lp');
    const roots = await explorer.roots();
    assert.equal(roots[0].icon, 'error');
    assert.match(roots[0].label, /boom/);
  });

  it('marks a truncated section', async () => {
    const client = new MockClient();
    client.model = () => Promise.resolve({ ...MODEL, truncated: { objectives: false, constraints: true, variables: false } });
    const explorer = new ModelExplorer(client);
    explorer.setDocument(MODEL.uri);
    const roots = await explorer.roots();
    assert.equal(roots.find((n) => n.label === 'Constraints')?.description, '2+');
  });

  it('reports an empty model', async () => {
    const client = new MockClient();
    client.model = () => Promise.resolve({ ...MODEL, objectives: [], constraints: [], variables: [] });
    const explorer = new ModelExplorer(client);
    explorer.setDocument(MODEL.uri);
    const roots = await explorer.roots();
    assert.equal(roots.length, 1);
    assert.equal(roots[0].label, 'The model is empty.');
  });
});

describe('ModelExplorer children', () => {
  it('groups variables by type in a fixed order', async () => {
    const { explorer } = await loaded();
    const variables = (await explorer.roots()).find((n) => n.label === 'Variables');
    assert.ok(variables);
    const groups = await explorer.children(variables);
    assert.deepEqual(
      groups.map((g) => [g.label, g.description]),
      [
        ['Continuous', '2'],
        ['Integer', '1'],
        ['Binary', '1'],
      ],
    );
    const continuous = await explorer.children(groups[0]);
    assert.deepEqual(
      continuous.map((v) => [v.label, v.description, v.expandable]),
      [
        ['x', '[0, +inf] · 3', true],
        ['z', 'unused', false],
      ],
    );
    const integer = await explorer.children(groups[1]);
    assert.equal(integer[0].description, 'free · 3');
  });

  it('adds entry descriptions to the tooltips and leaves other tooltips alone', async () => {
    const client = new MockClient();
    client.model = () =>
      Promise.resolve({
        ...MODEL,
        objectives: [{ name: 'obj', range: r(1), selectionRange: r(1), terms: 3, quadraticTerms: 0, description: 'Total cost' }],
        constraints: [constraint('c1', { description: 'Plant\u202E cap' }), constraint('c2')],
        variables: [variable('x', 'continuous', { description: 'Width' }), variable('z', 'continuous')],
      });
    const explorer = new ModelExplorer(client);
    explorer.setDocument(MODEL.uri);
    const roots = await explorer.roots();
    const section = (label: string) => roots.find((n) => n.label === label) as ExplorerNode;
    const [obj] = await explorer.children(section('Objectives'));
    assert.equal(obj.tooltip, 'Total cost');
    const [c1, c2] = await explorer.children(section('Constraints'));
    assert.equal(c1.tooltip, 'linear constraint, 2 terms\n\nPlant cap');
    assert.equal(c2.tooltip, 'linear constraint, 2 terms');
    const [group] = await explorer.children(section('Variables'));
    const [x, z] = await explorer.children(group);
    assert.equal(x.tooltip, 'continuous; 2 constraints, 1 objectives\n\nWidth');
    assert.equal(z.tooltip, 'continuous; 2 constraints, 1 objectives');
  });

  it('lists constraints with operator, rhs, range partner and source range', async () => {
    const { explorer } = await loaded();
    const section = (await explorer.roots()).find((n) => n.label === 'Constraints');
    assert.ok(section);
    const rows = await explorer.children(section);
    assert.deepEqual(
      rows.map((n) => [n.label, n.description]),
      [
        ['c1', '<= 100'],
        ['c2', '>= 5 · range with c2_rng'],
      ],
    );
    assert.deepEqual(rows[0].range, r(4));
  });

  it('describes special rows by kind or class', async () => {
    const { explorer } = await loaded();
    const section = (await explorer.roots()).find((n) => n.label === 'Special');
    assert.ok(section);
    const rows = await explorer.children(section);
    assert.deepEqual(
      rows.map((n) => [n.label, n.description]),
      [
        ['lim', '<= 100 · lazy'],
        ['s1', 'sos1'],
      ],
    );
  });

  it('expands a constraint into coefficients, with a click target and truncation notice', async () => {
    const { explorer, client } = await loaded();
    const section = (await explorer.roots()).find((n) => n.label === 'Constraints');
    assert.ok(section);
    const [c1] = await explorer.children(section);
    const terms = await explorer.children(c1);
    assert.deepEqual(client.explainCalls, [{ kind: 'constraint', name: 'c1' }]);
    assert.deepEqual(
      terms.map((t) => [t.kind, t.label]),
      [
        ['term', '2 x'],
        ['term', '-y'],
        ['term', 'w'],
        ['notice', 'More terms not shown.'],
      ],
    );
    assert.deepEqual(terms[0].range, r(4));
    assert.equal(terms[2].range, undefined);
  });

  it('expands a variable into the rows and objectives that use it', async () => {
    const { explorer, client } = await loaded();
    const variables = (await explorer.roots()).find((n) => n.label === 'Variables');
    assert.ok(variables);
    const [continuous] = await explorer.children(variables);
    const [x] = await explorer.children(continuous);
    const usages = await explorer.children(x);
    assert.deepEqual(client.explainCalls, [{ kind: 'variable', name: 'x' }]);
    assert.deepEqual(
      usages.map((u) => [u.label, u.description, u.icon]),
      [
        ['c1', 'linear (2)', 'symbol-operator'],
        ['obj', 'linear', 'target'],
      ],
    );
    assert.deepEqual(usages[0].range, r(4));
  });

  it('turns an explain failure into an error child', async () => {
    const { explorer, client } = await loaded();
    client.explain = () => Promise.reject(new Error('gone'));
    const section = (await explorer.roots()).find((n) => n.label === 'Objectives');
    assert.ok(section);
    const [obj] = await explorer.children(section);
    const children = await explorer.children(obj);
    assert.equal(children[0].icon, 'error');
    assert.match(children[0].label, /gone/);
  });

  it('drops a reply for a document that was switched away from', async () => {
    const { explorer, client } = await loaded();
    const section = (await explorer.roots()).find((n) => n.label === 'Objectives');
    assert.ok(section);
    const [obj] = await explorer.children(section);
    client.explain = async (uri, target) => {
      explorer.setDocument('file:///other.lp');
      return { uri, version: 1, kind: target.kind, name: target.name, range: null, markdown: '', terms: [] };
    };
    assert.deepEqual(await explorer.children(obj), []);
  });
});

describe('ModelExplorer filter', () => {
  it('keeps only matching entities, shows matched/total and hides empty sections', async () => {
    const { explorer } = await loaded();
    explorer.setFilter('  C1 ');
    const roots = await explorer.roots();
    assert.deepEqual(
      roots.map((n) => [n.label, n.description]),
      [['Constraints', '1/2']],
    );
  });

  it('filters variables inside their groups', async () => {
    const { explorer } = await loaded();
    explorer.setFilter('x');
    const roots = await explorer.roots();
    const variables = roots.find((n) => n.label === 'Variables');
    assert.ok(variables);
    assert.equal(variables.description, '1/4');
    const groups = await explorer.children(variables);
    assert.deepEqual(
      groups.map((g) => [g.label, g.description]),
      [['Continuous', '1/2']],
    );
  });

  it('says so when nothing matches, and clears with empty text', async () => {
    const { explorer } = await loaded();
    explorer.setFilter('nope');
    assert.match((await explorer.roots())[0].label, /No entity matches "nope"/);
    explorer.setFilter('');
    assert.equal((await explorer.roots()).length, 4);
  });
});

describe('formatCoefficient', () => {
  it('omits unit coefficients and flags non-finite ones', () => {
    assert.equal(formatCoefficient(1, 'x'), 'x');
    assert.equal(formatCoefficient(-1, 'x'), '-x');
    assert.equal(formatCoefficient(2.5, 'x'), '2.5 x');
    assert.equal(formatCoefficient(null, 'x'), '? x');
  });
});
