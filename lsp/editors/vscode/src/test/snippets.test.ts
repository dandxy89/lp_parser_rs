import * as assert from 'node:assert/strict';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { describe, it } from 'node:test';

interface Snippet {
  prefix: string;
  description: string;
  body: string[];
}

// Compiled to out/test/, so the extension root is two levels up.
const extensionRoot = path.join(__dirname, '..', '..');
const snippetsPath = path.join(extensionRoot, 'snippets', 'lp.json');

function loadSnippets(): Record<string, Snippet> {
  const parsed: unknown = JSON.parse(fs.readFileSync(snippetsPath, 'utf8'));
  assert.ok(typeof parsed === 'object' && parsed !== null && !Array.isArray(parsed), 'snippets file is a JSON object');
  return parsed as Record<string, Snippet>;
}

/** Tabstop indices in a body: `$1`, `${1:default}` and `${1|a,b|}` forms. */
function tabstops(body: string): number[] {
  const indices: number[] = [];
  for (const match of body.matchAll(/\$\{(\d+)[:|]|\$(\d+)/g)) {
    indices.push(Number(match[1] ?? match[2]));
  }
  return indices;
}

/** Expands a body the way an editor does with defaults: `${n:x}` and `${n|a,b|}` take the first text, bare `$n` is empty. */
function expandDefaults(body: string): string {
  return body
    .replace(/\$\{\d+:([^}]*)\}/g, '$1')
    .replace(/\$\{\d+\|([^,|}]*)[^|}]*\|\}/g, '$1')
    .replace(/\$\d+/g, '');
}

describe('snippets/lp.json', () => {
  const snippets = loadSnippets();
  const entries = Object.entries(snippets);

  it('contains the required snippet set', () => {
    const prefixes = entries.map(([, snippet]) => snippet.prefix);
    for (const required of [
      'lp-model',
      'lp-objective',
      'lp-constraint',
      'lp-bounds',
      'lp-binary',
      'lp-general',
      'lp-semi-continuous',
      'lp-sos1',
      'lp-sos2',
      'lp-indicator',
      'lp-quad-objective',
      'lp-quad-constraint',
      'lp-multi-obj',
      'lp-ranged',
      'lp-lazy',
    ]) {
      assert.ok(prefixes.includes(required), `missing snippet ${required}`);
    }
  });

  it('has unique prefixes and a description for every snippet', () => {
    const prefixes = entries.map(([, snippet]) => snippet.prefix);
    assert.equal(new Set(prefixes).size, prefixes.length, 'prefixes must be unique');
    for (const [name, snippet] of entries) {
      assert.match(snippet.prefix, /^lp-[a-z0-9-]+$/, `${name}: prefix`);
      assert.ok(snippet.description.length > 0, `${name}: description`);
    }
  });

  it('has a non-empty body of strings with balanced braces', () => {
    for (const [name, snippet] of entries) {
      assert.ok(Array.isArray(snippet.body) && snippet.body.length > 0, `${name}: body is a non-empty array`);
      for (const line of snippet.body) {
        assert.equal(typeof line, 'string', `${name}: body lines are strings`);
      }
      const text = snippet.body.join('\n');
      assert.equal(text.split('{').length, text.split('}').length, `${name}: balanced braces`);
    }
  });

  it('numbers tabstops contiguously from 1, with $0 optional', () => {
    for (const [name, snippet] of entries) {
      const indices = tabstops(snippet.body.join('\n'));
      const nonFinal = [...new Set(indices.filter((index) => index !== 0))].sort((a, b) => a - b);
      assert.ok(nonFinal.length > 0, `${name}: has at least one tabstop`);
      assert.deepEqual(
        nonFinal,
        nonFinal.map((_, position) => position + 1),
        `${name}: tabstops 1..n with no gaps`,
      );
      assert.ok(indices.filter((index) => index === 0).length <= 1, `${name}: at most one $0`);
    }
  });

  it('fills every placeholder with its default and leaves no placeholder syntax behind', () => {
    for (const [name, snippet] of entries) {
      const expanded = expandDefaults(snippet.body.join('\n'));
      assert.ok(!expanded.includes('$'), `${name}: placeholder left after expansion`);
      assert.ok(expanded.trim().length > 0, `${name}: expands to text`);
    }
  });
});
