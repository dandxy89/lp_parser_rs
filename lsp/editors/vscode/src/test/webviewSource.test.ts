import * as assert from 'node:assert/strict';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { describe, it } from 'node:test';
import { VIEW_NAMES } from '../pure/pages';

const src = path.join(__dirname, '..', '..', 'src');

function sources(directory: string): { file: string; text: string }[] {
  return fs
    .readdirSync(path.join(src, directory))
    .filter((name) => name.endsWith('.ts'))
    .map((name) => ({ file: `${directory}/${name}`, text: fs.readFileSync(path.join(src, directory, name), 'utf8') }));
}

// Model names are attacker-controlled (any `.lp` file). Webview code must reach the DOM only through
// `textContent` and `createElement`; these APIs parse strings as markup or code.
const FORBIDDEN: readonly [RegExp, string][] = [
  [/\.innerHTML\b/, 'innerHTML'],
  [/\.outerHTML\b/, 'outerHTML'],
  [/insertAdjacentHTML/, 'insertAdjacentHTML'],
  [/document\.write/, 'document.write'],
  [/\beval\s*\(/, 'eval'],
  [/new\s+Function\b/, 'new Function'],
  [/setTimeout\s*\(\s*['"`]/, 'setTimeout with a string'],
  [/setAttribute\(\s*['"]on/, 'setAttribute(on...)'],
  [/createContextualFragment|DOMParser/, 'string-to-DOM parsing'],
  [/\bfetch\s*\(|XMLHttpRequest|WebSocket|importScripts/, 'network access'],
  [/\.srcdoc\b|\.href\s*=|location\s*=/, 'navigation'],
];

describe('webview sources', () => {
  for (const { file, text } of sources('webviews')) {
    it(`${file} never parses strings as markup or code`, () => {
      for (const [pattern, label] of FORBIDDEN) {
        assert.ok(!pattern.test(text), `${file} uses ${label}`);
      }
    });
  }

  it('has one entry script per view, each posting ready and handling host messages', () => {
    for (const view of VIEW_NAMES) {
      const text = fs.readFileSync(path.join(src, 'webviews', `${view}.ts`), 'utf8');
      assert.ok(text.includes("post({ type: 'ready' })"), `${view}: announces ready`);
      assert.ok(text.includes('onHostMessage'), `${view}: handles host messages`);
    }
  });

  it('keeps pure modules free of vscode and DOM dependencies', () => {
    for (const { file, text } of sources('pure')) {
      assert.ok(!/from 'vscode'/.test(text), `${file} imports vscode`);
      assert.ok(!/\bdocument\.|\bwindow\./.test(text), `${file} touches the DOM`);
    }
  });

  it('host panels register webviews with a nonce-based CSP and restricted resource roots', () => {
    const panel = fs.readFileSync(path.join(src, 'views', 'panel.ts'), 'utf8');
    assert.ok(panel.includes('makeNonce'));
    assert.ok(panel.includes('localResourceRoots: [scripts]'));
    assert.ok(panel.includes('parseWebviewMessage'), 'inbound messages are validated');
  });
});
