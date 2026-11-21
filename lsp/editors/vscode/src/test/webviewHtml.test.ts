import * as assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { VIEW_NAMES, pageBody } from '../pure/pages';
import { buildPage, contentSecurityPolicy, isValidNonce, makeNonce } from '../pure/webviewHtml';
import { WEBVIEW_CSS } from '../pure/webviewCss';

const NONCE = makeNonce(() => new Uint8Array(16).fill(7));

describe('makeNonce', () => {
  it('encodes 16 bytes as base64url and passes validation', () => {
    assert.ok(isValidNonce(NONCE));
    assert.equal(NONCE.length, 22);
  });

  it('differs for different random bytes', () => {
    assert.notEqual(NONCE, makeNonce(() => new Uint8Array(16).fill(8)));
  });

  it('refuses a short random source', () => {
    assert.throws(() => makeNonce(() => new Uint8Array(4)));
  });
});

describe('contentSecurityPolicy', () => {
  const csp = contentSecurityPolicy(NONCE);

  it('allows nothing by default and scripts and styles only with the nonce', () => {
    assert.match(csp, /default-src 'none'/);
    assert.ok(csp.includes(`script-src 'nonce-${NONCE}'`));
    assert.ok(csp.includes(`style-src 'nonce-${NONCE}'`));
  });

  it('never allows inline, eval, network or remote sources', () => {
    assert.ok(!csp.includes('unsafe-inline'));
    assert.ok(!csp.includes('unsafe-eval'));
    assert.ok(!csp.includes('connect-src'));
    assert.ok(!/https?:|\*/.test(csp));
    assert.match(csp, /frame-ancestors 'none'/);
    assert.match(csp, /base-uri 'none'/);
  });

  it('rejects an empty or malformed nonce', () => {
    for (const bad of ['', 'short', "abc' 'unsafe-inline", 'x'.repeat(20) + ';']) {
      assert.throws(() => contentSecurityPolicy(bad));
    }
  });
});

describe('buildPage', () => {
  const base = { title: 'Matrix', nonce: NONCE, css: WEBVIEW_CSS, body: '<main></main>', scriptUri: 'https://file%2B.vscode-resource.vscode-cdn.net/x/matrix.js' };

  it('puts the nonce on the one script and the one style, and the CSP in a meta tag', () => {
    const html = buildPage(base);
    assert.equal(html.match(/<script /g)?.length, 1);
    assert.equal(html.match(/<style /g)?.length, 1);
    assert.ok(html.includes(`<script nonce="${NONCE}" src="${base.scriptUri}">`));
    assert.ok(html.includes(`<style nonce="${NONCE}">`));
    assert.match(html, /<meta http-equiv="Content-Security-Policy" content="default-src &#39;none&#39;/);
  });

  it('escapes the title', () => {
    const html = buildPage({ ...base, title: '</title><script>alert(1)</script>' });
    assert.ok(!html.includes('<script>alert'));
    assert.ok(html.includes('&lt;/title&gt;'));
  });

  it('rejects a script URI that could close the attribute', () => {
    for (const uri of ['x" onload="alert(1)', "x' y", 'x><script>', 'a b']) {
      assert.throws(() => buildPage({ ...base, scriptUri: uri }));
    }
  });

  it('rejects css that would close the style element', () => {
    assert.throws(() => buildPage({ ...base, css: 'a{}</style><script>x</script>' }));
  });
});

describe('page bodies', () => {
  for (const view of VIEW_NAMES) {
    it(`${view}: static markup with no script, handler attribute or external reference`, () => {
      const body = pageBody(view);
      assert.ok(!/<script/i.test(body));
      assert.ok(!/\son\w+\s*=/i.test(body), 'no inline event handlers');
      assert.ok(!/(src|href)\s*=/i.test(body), 'no external references');
      assert.ok(!/javascript:/i.test(body));
    });
  }

  it('every id the webview scripts read exists in the body', () => {
    // Guards against renaming a control in one place only: each `byId('x')` in a view script must be in its body.
    const fs = require('node:fs') as typeof import('node:fs');
    const path = require('node:path') as typeof import('node:path');
    for (const view of VIEW_NAMES) {
      const source = fs.readFileSync(path.join(__dirname, '..', '..', 'src', 'webviews', `${view}.ts`), 'utf8');
      const ids = [...source.matchAll(/byId(?:<[^>]+>)?\('([^']+)'\)/g)].map((m) => m[1]);
      assert.ok(ids.length > 0, `${view}: reads ids`);
      for (const id of ids) {
        assert.ok(pageBody(view).includes(`id="${id}"`), `${view}: #${id}`);
      }
    }
  });
});
