#!/usr/bin/env node
'use strict';
// A stand-in lp-lsp for the extension test host: speaks just enough LSP over stdio
// (Content-Length framing) to drive the views, with fixed answers for the model in
// workspace/model.lp. It is not a language server; the real one is tested in lsp/.

const REQUESTS = ['lp/model', 'lp/explain', 'lp/graph', 'lp/matrix', 'lp/diff', 'lp/solve', 'lp/solverInfo'];

const range = (line, start, end) => ({ start: { line, character: start }, end: { line, character: end } });

const MODEL = {
  version: null,
  summary: {
    name: null,
    sense: 'maximize',
    objectives: 1,
    constraints: 2,
    variables: 2,
    nonzeros: 4,
    density: 1,
    variablesByKind: { continuous: 2 },
    constraintsByKind: { linear: 2 },
    diagnostics: { error: 0, warning: 1, info: 1 },
  },
  objectives: [{ name: 'obj', range: range(1, 1, 16), selectionRange: range(1, 1, 4), terms: 2, quadraticTerms: 0 }],
  constraints: [
    { name: 'c1', generated: false, kind: 'linear', class: 'normal', operator: '<=', rhs: 10, terms: 2, rangePartner: null, range: range(3, 1, 16), selectionRange: range(3, 1, 3) },
    { name: 'c2', generated: false, kind: 'linear', class: 'normal', operator: '>=', rhs: 1, terms: 2, rangePartner: null, range: range(4, 1, 16), selectionRange: range(4, 1, 3) },
  ],
  variables: [
    { name: 'x', kind: 'continuous', bounds: { lower: 0, upper: 8 }, usage: { objectives: 1, constraints: 2, quadratic: 0 }, unused: false, free: false, range: range(1, 9, 10) },
    { name: 'y', kind: 'continuous', bounds: { lower: 0, upper: null }, usage: { objectives: 1, constraints: 2, quadratic: 0 }, unused: false, free: false, range: range(1, 16, 17) },
  ],
  truncated: { objectives: false, constraints: false, variables: false },
};

function model(params) {
  const include = params.include;
  const keep = (key) => include === undefined || include.includes(key);
  const out = { uri: params.textDocument.uri, version: MODEL.version, truncated: MODEL.truncated };
  for (const key of ['summary', 'objectives', 'constraints', 'variables']) {
    if (keep(key)) out[key] = MODEL[key];
  }
  return out;
}

function matrix(params) {
  return {
    mode: 'entries',
    version: null,
    uri: params.textDocument.uri,
    shape: { rows: 2, cols: 2, nonzeros: 4 },
    rowOffset: 0,
    colOffset: 0,
    rows: [
      { name: 'c1', kind: 'linear', operator: '<=', rhs: 10 },
      { name: 'c2', kind: 'linear', operator: '>=', rhs: 1 },
    ],
    cols: [
      { name: 'x', kind: 'continuous' },
      { name: 'y', kind: 'continuous' },
    ],
    entries: { row: [0, 0, 1, 1], col: [0, 1, 0, 1], value: [1, 1, 1, -1] },
    scale: { minAbs: 1, maxAbs: 1 },
  };
}

function explain(params) {
  const target = params.target ?? { kind: 'constraint', name: 'c1' };
  return {
    uri: params.textDocument.uri,
    version: null,
    kind: target.kind,
    name: target.name,
    range: range(3, 1, 16),
    markdown: `**${target.name}**\n\nx + y <= 10`,
    terms: [
      { variable: 'x', coefficient: 1, range: range(3, 5, 6) },
      { variable: 'y', coefficient: 1, range: range(3, 9, 10) },
    ],
    termsTruncated: false,
    operator: '<=',
    rhs: 10,
    related: [],
    usage: [],
  };
}

function diagnostics(uri) {
  const diagnostic = (code, severity, line, message, category, certainty) => ({
    range: range(line, 1, 5),
    severity,
    code,
    source: 'lp-lsp',
    message,
    data: { category, ...(certainty === undefined ? {} : { certainty }) },
  });
  return {
    uri,
    diagnostics: [
      diagnostic('analysis/unused-variable', 2, 1, 'variable is never used', 'structural'),
      diagnostic('solver/iis-member', 2, 3, 'c1 is part of the infeasible core', 'solver', 'proven'),
    ],
  };
}

const versions = new Map();

function solve(params) {
  return {
    status: 'optimal',
    version: versions.get(params.textDocument.uri) ?? null,
    objectiveValue: 26,
    solver: { name: 'standin', version: '0' },
    timings: { buildMs: 0, solveMs: 0, extractMs: 0 },
    variables: [{ name: 'x', value: 8 }, { name: 'y', value: 1 }],
    constraints: [{ name: 'c1', activity: 9, slack: 1, binding: false }],
  };
}

function send(message) {
  const body = Buffer.from(JSON.stringify({ jsonrpc: '2.0', ...message }), 'utf8');
  process.stdout.write(`Content-Length: ${body.length}\r\n\r\n`);
  process.stdout.write(body);
}

function handle(message) {
  const { id, method, params } = message;
  const reply = (result) => id !== undefined && send({ id, result });
  switch (method) {
    case 'initialize':
      return reply({
        capabilities: { textDocumentSync: 1, experimental: { lp: { requests: REQUESTS } } },
        serverInfo: { name: 'lp-lsp-standin', version: '0.0.0' },
      });
    case 'textDocument/didOpen':
    case 'textDocument/didChange':
      versions.set(params.textDocument.uri, params.textDocument.version);
      return send({ method: 'textDocument/publishDiagnostics', params: diagnostics(params.textDocument.uri) });
    case 'lp/model':
      return reply(model(params));
    case 'lp/matrix':
      return reply(matrix(params));
    case 'lp/explain':
      return reply(explain(params));
    case 'lp/solve':
      return reply(solve(params));
    case 'shutdown':
      return reply(null);
    case 'exit':
      return process.exit(0);
    default:
      if (id !== undefined) send({ id, error: { code: -32601, message: `method not found: ${method}` } });
  }
}

let buffer = Buffer.alloc(0);
process.stdin.on('data', (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  for (;;) {
    const split = buffer.indexOf('\r\n\r\n');
    if (split < 0) return;
    const match = /Content-Length: (\d+)/i.exec(buffer.subarray(0, split).toString('ascii'));
    if (match === null) throw new Error('missing Content-Length');
    const length = Number(match[1]);
    const end = split + 4 + length;
    if (buffer.length < end) return;
    const message = JSON.parse(buffer.subarray(split + 4, end).toString('utf8'));
    buffer = buffer.subarray(end);
    handle(message);
  }
});
process.stdin.on('end', () => process.exit(0));
