import * as assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { AnalysisDiagnostic, groupByCategory, readDiagnosticData } from '../pure/analysis';

const at = (line: number) => ({ start: { line, character: 0 }, end: { line, character: 1 } });
const d = (code: string, category: string | undefined, severity: 1 | 2 | 3 | 4, line: number): AnalysisDiagnostic => ({
  code,
  message: `${code} at ${line}`,
  severity,
  category,
  certainty: category === 'numerical' ? 'heuristic' : 'proven',
  range: at(line),
});

describe('groupByCategory', () => {
  it('returns nothing for no diagnostics', () => {
    assert.deepEqual(groupByCategory([]), []);
  });

  it('orders categories, then codes by severity, and counts findings', () => {
    const tree = groupByCategory([
      d('large-coefficient', 'numerical', 3, 5),
      d('duplicate-row', 'structural', 2, 9),
      d('unused-variable', 'structural', 3, 2),
      d('unused-variable', 'structural', 3, 1),
      d('invalid-bounds', 'mathematical', 1, 3),
      d('mystery', undefined, 2, 0),
      d('custom', 'zzz', 2, 0),
    ]);
    assert.deepEqual(
      tree.map((n) => [n.label, n.description]),
      [
        ['Structural', '3'],
        ['Numerical', '1'],
        ['Mathematical', '1'],
        ['zzz', '1'],
        ['Other', '1'],
      ],
    );
    const structural = tree[0];
    assert.deepEqual(
      structural.children.map((c) => [c.label, c.description]),
      [
        ['duplicate-row', '1'],
        ['unused-variable', '2'],
      ],
    );
    const unused = structural.children[1];
    assert.deepEqual(
      unused.children.map((f) => f.range?.start.line),
      [1, 2],
    );
    assert.equal(unused.children[0].description, 'proven');
  });

  it('takes the worst severity for the code icon and gives unique ids', () => {
    const tree = groupByCategory([d('a', 'structural', 3, 1), d('a', 'structural', 1, 2), d('b', 'structural', 2, 3)]);
    const ids = new Set<string>();
    const walk = (nodes: readonly { id: string; children: readonly never[] }[]): void => {
      for (const n of nodes) {
        assert.ok(!ids.has(n.id), `duplicate id ${n.id}`);
        ids.add(n.id);
        walk(n.children as never);
      }
    };
    walk(tree as never);
    assert.equal(tree[0].children[0].icon, 'error');
    assert.equal(ids.size, 1 + 2 + 3);
  });
});

describe('readDiagnosticData', () => {
  it('reads category and certainty', () => {
    assert.deepEqual(readDiagnosticData({ category: 'solver', certainty: 'proven' }), { category: 'solver', certainty: 'proven' });
  });
  it('ignores malformed data', () => {
    const none = { category: undefined, certainty: undefined };
    assert.deepEqual(readDiagnosticData(undefined), none);
    assert.deepEqual(readDiagnosticData('x'), none);
    assert.deepEqual(readDiagnosticData({ category: 3 }), none);
  });
});
