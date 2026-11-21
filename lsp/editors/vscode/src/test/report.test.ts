import * as assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import type { AnalysisDiagnostic } from '../pure/analysis';
import type { ModelResult } from '../pure/modelTypes';
import { escapeMarkdown, renderModelReport, reportFileName } from '../pure/report';

const range = (line: number) => ({ start: { line, character: 0 }, end: { line, character: 1 } });

const MODEL: ModelResult = {
  uri: 'file:///model.lp',
  version: 1,
  summary: {
    name: 'plan',
    sense: 'maximize',
    objectives: 1,
    constraints: 3,
    variables: 3,
    nonzeros: 6,
    density: 0.6667,
    variablesByKind: { continuous: 1, binary: 1, integer: 0, semiContinuous: 1 },
    constraintsByKind: { linear: 3 },
    diagnostics: { error: 0, warning: 2, info: 1 },
  },
  objectives: [{ name: 'obj', range: null, selectionRange: null, terms: 3, quadraticTerms: 0 }],
  constraints: [
    { name: 'c1', generated: false, kind: 'linear', class: 'normal', operator: '<=', rhs: 10, terms: 2, rangePartner: null, range: null, selectionRange: null },
    { name: 'c2', generated: true, kind: 'linear', class: 'normal', operator: '>=', rhs: 1, terms: 2, rangePartner: null, range: null, selectionRange: null },
    { name: 'c3', generated: false, kind: 'linear', class: 'lazy', operator: '=', rhs: null, terms: 2, rangePartner: null, range: null, selectionRange: null },
  ],
  variables: [
    { name: 'b', kind: 'binary', bounds: { lower: 0, upper: 1 }, usage: { objectives: 1, constraints: 2, quadratic: 0 }, unused: false, free: false, range: null },
    { name: 'f', kind: 'continuous', bounds: { lower: null, upper: null }, usage: { objectives: 1, constraints: 2, quadratic: 0 }, unused: true, free: true, range: null },
    { name: 'x', kind: 'semiContinuous', bounds: { lower: 0, upper: 10 }, usage: { objectives: 1, constraints: 2, quadratic: 0 }, unused: false, free: false, range: null },
  ],
  truncated: { objectives: false, constraints: false, variables: false },
};

const finding = (code: string, line: number, message: string, category?: string, certainty?: string): AnalysisDiagnostic => ({
  code,
  message,
  severity: 2,
  category,
  certainty,
  range: range(line),
});

const DIAGNOSTICS = [
  finding('analysis/unused-variable', 4, 'variable f is never used', 'structural'),
  finding('analysis/large-coefficient', 6, 'coefficient 1e9 is large', 'numerical'),
  finding('custom/other', 1, 'something else'),
];

const EXPECTED = `# Model report: model\\.lp

Model name: plan

## Statistics

| Statistic | Value |
| --- | --- |
| Sense | maximize |
| Objectives | 1 |
| Constraints | 3 |
| Variables | 3 |
| Non-zeros | 6 |
| Density | 66.67% |
| Errors | 0 |
| Warnings | 2 |
| Information | 1 |

## Objectives

| Name | Terms |
| --- | --- |
| obj | 3 |

## Constraints

| Name | Kind | Class | Sense | RHS | Terms |
| --- | --- | --- | --- | --- | --- |
| c1 | linear | normal | \\<= | 10 | 2 |
| c2 (generated) | linear | normal | \\>= | 1 | 2 |
| c3 | linear | lazy | = | – | 2 |

## Bounds

| Variable | Lower | Upper |
| --- | --- | --- |
| b | 0 | 1 |
| f | -inf | +inf |
| x | 0 | 10 |

## Variable types

| Type | Count |
| --- | --- |
| binary | 1 |
| continuous | 1 |
| semiContinuous | 1 |

Unused variables: 1

## Warnings

### Structural (1)

- **analysis/unused\\-variable** (1)
  - Line 5: variable f is never used

### Numerical (1)

- **analysis/large\\-coefficient** (1)
  - Line 7: coefficient 1e9 is large

### Other (1)

- **custom/other** (1)
  - Line 2: something else
`;

describe('renderModelReport', () => {
  it('renders a full report', () => {
    assert.equal(renderModelReport({ fileName: 'model.lp', model: MODEL, diagnostics: DIAGNOSTICS }), EXPECTED);
  });

  it('escapes markdown in names and messages', () => {
    const model = { ...MODEL, constraints: [{ ...MODEL.constraints![0], name: 'a|b_*c*' }] } as ModelResult;
    const md = renderModelReport({
      fileName: 'm.lp',
      model,
      diagnostics: [finding('x/y', 0, '<script>[x](command:evil)\nsecond line', 'structural')],
    });
    const row = md.split('\n').find((line) => line.includes('c\\*')) ?? '';
    assert.ok(row.startsWith('| a\\|b\\_\\*c\\* |'), row);
    assert.ok(!md.includes('<script>'));
    assert.ok(!md.includes('](command:'));
    assert.ok(md.includes('Line 1: \\<script\\>\\[x\\]\\(command:evil\\) second line'));
  });

  it('omits the description column when absent and includes it when present', () => {
    const plain = renderModelReport({ fileName: 'm.lp', model: MODEL, diagnostics: [] });
    assert.ok(!plain.includes('Description'));
    const model = {
      ...MODEL,
      description: 'Weekly plan',
      constraints: [{ ...MODEL.constraints![0], description: 'Capacity | limit' }, MODEL.constraints![1]],
      variables: [{ ...MODEL.variables![0], description: 'On or off' }],
    } as unknown as ModelResult;
    const md = renderModelReport({ fileName: 'm.lp', model, diagnostics: [] });
    assert.ok(md.includes('Weekly plan'));
    assert.ok(md.includes('| Name | Kind | Class | Sense | RHS | Terms | Description |'));
    assert.ok(md.includes('Capacity \\| limit'));
    assert.ok(md.includes('| Variable | Type | Description |'));
    assert.ok(md.includes('| b | binary | On or off |'));
    assert.ok(!md.includes('| Name | Terms | Description |'), 'objectives have no descriptions');
  });

  it('handles missing summary and lists', () => {
    const model = { uri: 'file:///m.lp', version: null, truncated: { objectives: false, constraints: false, variables: false } } as ModelResult;
    const md = renderModelReport({ fileName: 'm.lp', model, diagnostics: [] });
    assert.ok(md.includes('Not reported by the server.'));
    assert.ok(md.includes('No findings.'));
  });

  it('renders bad numbers as a dash and an empty model as None', () => {
    const model = {
      ...MODEL,
      summary: { ...MODEL.summary, nonzeros: Number.NaN, density: 'x' },
      objectives: [],
      constraints: [{ ...MODEL.constraints![0], rhs: 'oops', terms: Number.POSITIVE_INFINITY }],
    } as unknown as ModelResult;
    const md = renderModelReport({ fileName: 'm.lp', model, diagnostics: [] });
    assert.ok(md.includes('| Non-zeros | – |'));
    assert.ok(md.includes('| Density | – |'));
    assert.ok(md.includes('## Objectives\n\nNone.'));
    assert.ok(md.includes('| c1 | linear | normal | \\<= | – | – |'));
  });

  it('notes truncation', () => {
    const model = { ...MODEL, truncated: { objectives: false, constraints: true, variables: false } };
    assert.ok(renderModelReport({ fileName: 'm.lp', model, diagnostics: [] }).includes('> Lists are truncated by the server'));
    assert.ok(!EXPECTED.includes('truncated'));
  });

  it('lists only non-default bounds and renders infinities', () => {
    const model = {
      ...MODEL,
      variables: [
        { ...MODEL.variables![0], bounds: { lower: 0, upper: null } },
        { ...MODEL.variables![1], bounds: { lower: null, upper: 5 } },
      ],
    };
    const md = renderModelReport({ fileName: 'm.lp', model, diagnostics: [] });
    assert.ok(md.includes('| f | -inf | 5 |'));
    assert.ok(!md.includes('| b |'));
    const defaults = { ...MODEL, variables: [{ ...MODEL.variables![0], bounds: { lower: 0, upper: null } }] };
    assert.ok(renderModelReport({ fileName: 'm.lp', model: defaults, diagnostics: [] }).includes('All variables use the default bounds [0, +inf).'));
  });
});

describe('reportFileName and escapeMarkdown', () => {
  it('derives the report name', () => {
    assert.equal(reportFileName('model.lp'), 'model.report.md');
    assert.equal(reportFileName('x.LP'), 'x.report.md');
    assert.equal(reportFileName('noext'), 'noext.report.md');
  });

  it('escapes syntax characters and flattens line breaks', () => {
    assert.equal(escapeMarkdown('a|b\r\nc'), 'a\\|b c');
    assert.equal(escapeMarkdown('&<'), '&amp;\\<');
  });
});
