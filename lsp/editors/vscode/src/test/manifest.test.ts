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
  activationEvents: string[];
  contributes: {
    configuration: { properties: Record<string, { type?: string; default?: unknown }> };
    keybindings: { command: string; key: string; mac?: string; when: string }[];
    walkthroughs: {
      id: string;
      steps: { id: string; description: string; media: { markdown: string }; completionEvents: string[] }[];
    }[];
    languages: { id: string; extensions: string[]; configuration: string }[];
    grammars: { language: string; scopeName: string; path: string }[];
    commands: ContributedCommand[];
    snippets: { language: string; path: string }[];
    menus: {
      commandPalette: { command: string; when?: string }[];
      'editor/context': { command: string; when?: string; group?: string }[];
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

  it('contributes Find Similar Constraints without enablement, in the palette and the editor menu', () => {
    const command = 'lp.findSimilarConstraints';
    const contributed = manifest.contributes.commands.find((entry) => entry.command === command);
    assert.equal(contributed?.title, 'Find Similar Constraints');
    assert.equal(contributed?.category, 'LP');
    assert.equal(contributed?.enablement, undefined);
    assert.equal(manifest.contributes.menus.commandPalette.find((item) => item.command === command)?.when, 'editorLangId == lp');
    const context = manifest.contributes.menus['editor/context'].find((item) => item.command === command);
    assert.deepEqual([context?.when, context?.group], ['editorLangId == lp', 'lp@3']);
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
    const report = fs.readFileSync(path.join(extensionRoot, 'src', 'report.ts'), 'utf8');
    for (const feature of FEATURE_COMMANDS) {
      const registered = (text: string): boolean => new RegExp(`registerCommand\\(\\s*'${feature.command}'`).test(text);
      assert.ok(registered(views) || registered(explorer) || registered(report), `${feature.command} is registered`);
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

  it('registers the mps language, grammar and configuration from files that exist', () => {
    const language = manifest.contributes.languages.find((entry) => entry.id === 'mps');
    assert.ok(language, 'mps language');
    assert.ok(language.extensions.includes('.mps'));
    const grammar = manifest.contributes.grammars.find((entry) => entry.language === 'mps');
    assert.equal(grammar?.scopeName, 'source.mps');
    for (const file of [language.configuration, grammar?.path ?? '']) {
      assert.ok(fs.existsSync(path.join(extensionRoot, file)), `${file} exists`);
      JSON.parse(fs.readFileSync(path.join(extensionRoot, file), 'utf8'));
    }
  });

  it('toggles MPS comments at column 0 because the parser only accepts a leading *', () => {
    const language = manifest.contributes.languages.find((entry) => entry.id === 'mps');
    assert.ok(language);
    const config = JSON.parse(fs.readFileSync(path.join(extensionRoot, language.configuration), 'utf8')) as {
      comments: { lineComment: { comment: string; noIndent?: boolean } };
    };
    assert.equal(config.comments.lineComment.comment, '*');
    assert.equal(config.comments.lineComment.noIndent, true);
  });

  it('activates on mps documents', () => {
    assert.ok(manifest.activationEvents.includes('onLanguage:mps'));
  });

  it('contributes Convert from MPS in the palette for MPS documents', () => {
    const entry = manifest.contributes.menus.commandPalette.find((item) => item.command === 'lp.convertFromMps');
    assert.equal(entry?.when, 'editorLangId == mps');
    assert.ok(manifest.contributes.commands.some((item) => item.command === 'lp.convertFromMps'));
  });

  it('binds Solve, Explain and Show Model Explorer for LP editors only', () => {
    const contributed = new Set(manifest.contributes.commands.map((entry) => entry.command));
    const bound = manifest.contributes.keybindings;
    assert.deepEqual(bound.map((entry) => entry.command).sort(), ['lp.explainConstraint', 'lp.showModelExplorer', 'lp.solve']);
    for (const entry of bound) {
      assert.ok(entry.when.includes('editorLangId == lp'), `${entry.command}: scoped to LP editors`);
      assert.ok(contributed.has(entry.command), `${entry.command} is contributed`);
    }
    for (const field of ['key', 'mac'] as const) {
      const keys = bound.map((entry) => entry[field]);
      assert.equal(new Set(keys).size, keys.length, `no duplicate ${field} bindings`);
    }
  });

  it('has walkthrough steps with existing markdown and valid command links', () => {
    const walkthrough = manifest.contributes.walkthroughs.find((entry) => entry.id === 'lp.gettingStarted');
    assert.ok(walkthrough, 'walkthrough');
    assert.equal(new Set(walkthrough.steps.map((step) => step.id)).size, 6);
    const commands = new Set(manifest.contributes.commands.map((entry) => entry.command));
    const views = new Set(Object.values(manifest.contributes.views).flat().map((view) => view.id));
    const allowed = new Set(['workbench.action.files.newUntitledFile', 'workbench.action.openSettings']);
    const linkOk = (id: string): boolean =>
      commands.has(id) || allowed.has(id) || [...views].some((view) => id === `${view}.focus`);
    for (const step of walkthrough.steps) {
      const file = path.join(extensionRoot, step.media.markdown);
      assert.ok(fs.existsSync(file), `${step.media.markdown} exists`);
      const text = `${step.description}\n${fs.readFileSync(file, 'utf8')}`;
      for (const match of text.matchAll(/command:([A-Za-z0-9_.]+)/g)) {
        assert.ok(linkOk(match[1]), `${step.id}: command link ${match[1]}`);
      }
      for (const event of step.completionEvents) {
        const [kind, id] = event.split(':');
        if (kind === 'onCommand') {
          assert.ok(commands.has(id), `${step.id}: ${event}`);
        } else if (kind === 'onView') {
          assert.ok(views.has(id), `${step.id}: ${event}`);
        } else {
          assert.equal(event, 'onLanguage:lp');
        }
      }
    }
  });
});
