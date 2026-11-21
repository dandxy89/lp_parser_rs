import * as assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { chooseServer, ServerInputs } from '../pure/serverPath';

const base: ServerInputs = { configured: '', bundled: '/ext/server/lp-lsp', bundledExists: true, trusted: true };

describe('chooseServer', () => {
  it('prefers the trimmed setting in a trusted workspace', () => {
    assert.deepEqual(chooseServer({ ...base, configured: '  /opt/lp-lsp ' }), { command: '/opt/lp-lsp', source: 'setting' });
  });

  it('falls back to the bundled binary, then PATH, when trusted', () => {
    assert.deepEqual(chooseServer({ ...base, configured: '   ' }), { command: '/ext/server/lp-lsp', source: 'bundled' });
    assert.deepEqual(chooseServer({ ...base, bundledExists: false }), { command: 'lp-lsp', source: 'PATH' });
  });

  it('ignores the setting in Restricted Mode', () => {
    assert.deepEqual(chooseServer({ ...base, configured: '/opt/lp-lsp', trusted: false }), {
      command: '/ext/server/lp-lsp',
      source: 'bundled',
    });
  });

  it('runs nothing in Restricted Mode without a bundled binary', () => {
    assert.equal(chooseServer({ ...base, configured: '/opt/lp-lsp', bundledExists: false, trusted: false }), undefined);
    assert.equal(chooseServer({ ...base, bundledExists: false, trusted: false }), undefined);
  });

  it('never picks the setting or PATH in Restricted Mode', () => {
    for (const configured of ['', '/opt/lp-lsp']) {
      for (const bundledExists of [true, false]) {
        const source = chooseServer({ configured, bundled: '/b', bundledExists, trusted: false })?.source;
        assert.ok(source === undefined || source === 'bundled', `${configured}/${bundledExists}: ${String(source)}`);
      }
    }
  });
});
