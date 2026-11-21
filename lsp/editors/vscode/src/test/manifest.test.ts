import * as assert from 'node:assert/strict';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { describe, it } from 'node:test';
import { supportsContextKey } from '../pure/capabilities';
import { FEATURE_COMMANDS } from '../pure/commandTable';
import { VIEW_NAMES } from '../pure/pages';

interface ContributedCommand {
  command: string;
  title: string;
  category?: string;
  enablement?: string;
}

interface Manifest {
  capabilities: { untrustedWorkspaces: { supported: string; restrictedConfigurations: string[] } };
  contributes: {
    configuration: { properties: Record<string, { type?: string; default?: unknown }> };
    commands: ContributedCommand[];
    snippets: { language: string; path: string }[];
    menus: {
      commandPalette: { command: string; when?: string }[];
      'view/title': { command: string; when: string }[];
    };
    viewsContainers: { activitybar: { id: string; icon: string }[] };
    views: Record<string, { id: string; name: string }[]>;
  };
  scripts: Record<string, string>;
}

const extensionRoot = path.join(__dirname, '..', '..');
const manifest = JSON.parse(fs.readFileSync(path.join(extensionRoot, 'package.json'), 'utf8')) as Manifest;

describe('package.json contributions', () => {
  it('registers every feature command with an enablement gated on its request', () => {
    for (const feature of FEATURE_COMMANDS) {
      const contributed = manifest.contributes.commands.find((entry) => entry.command === feature.command);
      assert.ok(contributed, `${feature.command} is contributed`);
      assert.equal(contributed.title, feature.title, `${feature.command}: title`);
      assert.equal(contributed.enablement, supportsContextKey(feature.request), `${feature.command}: enablement`);
    }
  });

  it('gives every contributed command a title and category', () => {
    for (const entry of manifest.contributes.commands) {
      assert.ok(entry.title.length > 0, `${entry.command}: title`);
      assert.equal(entry.category, 'LP', `${entry.command}: category`);
    }
  });

  it('lists the feature commands in the command palette for LP documents', () => {
    const palette = manifest.contributes.menus.commandPalette;
    for (const feature of FEATURE_COMMANDS) {
      const entry = palette.find((item) => item.command === feature.command);
      assert.equal(entry?.when, 'editorLangId == lp', `${feature.command}: palette entry`);
    }
  });

  it('registers snippets for the lp language from a file that exists', () => {
    const lpSnippets = manifest.contributes.snippets.filter((entry) => entry.language === 'lp');
    assert.equal(lpSnippets.length, 1);
    assert.ok(fs.existsSync(path.join(extensionRoot, lpSnippets[0].path)), 'snippet file exists');
  });

  it('declares the LP activity bar container with the explorer and analysis views', () => {
    const container = manifest.contributes.viewsContainers.activitybar.find((entry) => entry.id === 'lp');
    assert.ok(container, 'activity bar container');
    assert.ok(fs.existsSync(path.join(extensionRoot, container.icon)), 'container icon exists');
    assert.deepEqual(
      manifest.contributes.views.lp.map((view) => view.id),
      ['lp.modelExplorer', 'lp.analysis'],
    );
  });

  it('only puts contributed commands in the explorer toolbar, and hides the internal reveal command', () => {
    const known = new Set(manifest.contributes.commands.map((entry) => entry.command));
    for (const item of manifest.contributes.menus['view/title']) {
      assert.ok(known.has(item.command), `${item.command} is contributed`);
      assert.equal(item.when, 'view == lp.modelExplorer');
    }
    const reveal = manifest.contributes.menus.commandPalette.find((item) => item.command === 'lp.revealRange');
    assert.equal(reveal?.when, 'false');
  });

  it('gates the explorer commands on lp/model support', () => {
    for (const id of ['lp.showModelExplorer', 'lp.explorer.refresh', 'lp.explorer.filter', 'lp.explorer.clearFilter']) {
      const entry = manifest.contributes.commands.find((item) => item.command === id);
      assert.equal(entry?.enablement, supportsContextKey('lp/model'), `${id}: enablement`);
    }
  });

  it('implements every feature command in the views module', () => {
    const views = fs.readFileSync(path.join(extensionRoot, 'src', 'views', 'index.ts'), 'utf8');
    const explorer = fs.readFileSync(path.join(extensionRoot, 'src', 'modelViews.ts'), 'utf8');
    for (const feature of FEATURE_COMMANDS) {
      const registered = (text: string): boolean => new RegExp(`registerCommand\\(\\s*'${feature.command}'`).test(text);
      assert.ok(registered(views) || registered(explorer), `${feature.command} is registered`);
    }
  });

  it('bundles one browser script per webview before packaging', () => {
    for (const script of ['compile', 'vscode:prepublish']) {
      assert.ok(manifest.scripts[script].includes('build:webviews'), `${script} builds the webviews`);
    }
    for (const view of VIEW_NAMES) {
      assert.ok(manifest.scripts['build:webviews'].includes(`src/webviews/${view}.ts`), `${view} is bundled`);
    }
    assert.ok(manifest.scripts['check-types'].includes('tsconfig.webviews.json'), 'webview sources are type-checked');
  });

  it('does not exclude the bundled webview scripts from the package', () => {
    const ignore = fs.readFileSync(path.join(extensionRoot, '.vscodeignore'), 'utf8').split('\n');
    assert.ok(!ignore.some((line) => line.trim() === 'dist/**' || line.trim() === 'dist/webviews/**'));
  });

  it('adds no runtime dependencies beyond the language client', () => {
    const raw = JSON.parse(fs.readFileSync(path.join(extensionRoot, 'package.json'), 'utf8')) as { dependencies: Record<string, string> };
    assert.deepEqual(Object.keys(raw.dependencies), ['vscode-languageclient']);
  });

  it('supports limited workspace trust and restricts every executable path setting', () => {
    const { supported, restrictedConfigurations } = manifest.capabilities.untrustedWorkspaces;
    assert.equal(supported, 'limited');
    const properties = manifest.contributes.configuration.properties;
    for (const key of Object.keys(properties).filter((name) => name.endsWith('.path'))) {
      assert.ok(restrictedConfigurations.includes(key), `${key} is a restricted configuration`);
    }
    for (const key of restrictedConfigurations) {
      assert.ok(key in properties, `${key} is a contributed setting`);
    }
  });

  it('contributes lp.codeLens.actions as a boolean on by default', () => {
    const setting = manifest.contributes.configuration.properties['lp.codeLens.actions'];
    assert.equal(setting?.type, 'boolean');
    assert.equal(setting?.default, true);
  });
});
