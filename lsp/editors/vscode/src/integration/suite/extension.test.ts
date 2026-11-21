import * as assert from 'node:assert';
import * as path from 'node:path';
import * as vscode from 'vscode';
import type { LpExtensionApi } from '../../extension';
import type { PanelEvent } from '../../views/panel';

const EXTENSION_ID = 'dandxy89.lp-lsp';
const FIXTURE = path.resolve(__dirname, '..', '..', '..', 'test-fixtures', 'workspace', 'model.lp');

async function waitFor<T>(what: string, probe: () => Promise<T | undefined> | T | undefined, timeoutMs = 20_000): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = await probe();
    if (value !== undefined) {
      return value;
    }
    if (Date.now() > deadline) {
      throw new Error(`Timed out waiting for ${what}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}

suite('LP extension host', () => {
  let api: LpExtensionApi;
  let editor: vscode.TextEditor;
  const events: PanelEvent[] = [];

  suiteSetup(async () => {
    const document = await vscode.workspace.openTextDocument(FIXTURE);
    editor = await vscode.window.showTextDocument(document);
    assert.strictEqual(document.languageId, 'lp');
    const extension = vscode.extensions.getExtension<LpExtensionApi>(EXTENSION_ID);
    assert.ok(extension, `extension ${EXTENSION_ID} is installed`);
    api = await extension.activate();
    api.observePanels((event) => events.push(event));
  });

  suiteTeardown(async () => {
    api.observePanels(undefined);
    await vscode.commands.executeCommand('workbench.action.closeAllEditors');
  });

  test('registers the contributed commands once the server advertises its requests', async () => {
    const wanted = [
      'lp.showModelExplorer',
      'lp.explorer.refresh',
      'lp.explorer.filter',
      'lp.explorer.clearFilter',
      'lp.revealRange',
      'lp.showMatrix',
      'lp.showDependencyGraph',
      'lp.compareModels',
      'lp.solve',
      'lp.diagnoseInfeasibility',
      'lp.explainConstraint',
      'lp.explainVariable',
      'lp.explainEntity',
      'lp.restartServer',
    ];
    const registered = new Set(await vscode.commands.getCommands(true));
    const missing = wanted.filter((id) => !registered.has(id));
    assert.deepStrictEqual(missing, []);
  });

  test('Model Explorer shows the sections of the fixture model', async () => {
    const roots = await waitFor('explorer roots', async () => {
      const nodes = await api.explorerRoots();
      return nodes.some((node) => node.kind === 'section') ? nodes : undefined;
    });
    const labels = roots.map((node) => node.label.toLowerCase());
    for (const section of ['objectives', 'constraints', 'variables']) {
      assert.ok(
        labels.some((label) => label.startsWith(section)),
        `a ${section} section in ${JSON.stringify(labels)}`,
      );
    }
    assert.ok(roots.every((node) => node.kind !== 'notice'), 'no error notice');
  });

  test('Analysis view groups server diagnostics by data.category, including solver', async () => {
    const tree = await waitFor('analysis categories', () => {
      const nodes = api.analysisTree();
      return nodes.some((node) => node.id === 'category:solver') ? nodes : undefined;
    });
    const ids = tree.map((node) => node.id);
    assert.deepStrictEqual(ids, ['category:structural', 'category:solver']);
    const solver = tree.find((node) => node.id === 'category:solver');
    assert.ok(solver);
    assert.strictEqual(solver.children[0]?.label, 'solver/iis-member');
    assert.strictEqual(solver.children[0]?.children[0]?.description, 'proven', 'certainty is read from data');
  });

  test('the matrix panel posts ready and receives data', async () => {
    events.length = 0;
    await vscode.window.showTextDocument(editor.document);
    await vscode.commands.executeCommand('lp.showMatrix');
    await waitFor('matrix ready and data', () => {
      const ready = events.findIndex((e) => e.view === 'matrix' && e.direction === 'in' && e.type === 'ready');
      const data = events.findIndex((e) => e.view === 'matrix' && e.direction === 'out' && e.type === 'data');
      return ready >= 0 && data > ready ? true : undefined;
    });
    assert.ok(
      !events.some((e) => e.view === 'matrix' && e.direction === 'out' && e.type === 'error'),
      'no error posted to the matrix panel',
    );
  });

  test('the explain panel loads the constraint it is opened for', async () => {
    events.length = 0;
    await vscode.commands.executeCommand('lp.explainEntity', editor.document.uri.toString(), 'constraint', 'c1');
    await waitFor('explain ready and data', () => {
      const ready = events.findIndex((e) => e.view === 'explain' && e.direction === 'in' && e.type === 'ready');
      const data = events.findIndex((e) => e.view === 'explain' && e.direction === 'out' && e.type === 'data');
      return ready >= 0 && data > ready ? true : undefined;
    });
  });

  test('revealRange selects the range in the editor', async () => {
    const range = { start: { line: 3, character: 1 }, end: { line: 3, character: 3 } };
    await vscode.commands.executeCommand('lp.revealRange', editor.document.uri.toString(), range);
    const active = vscode.window.activeTextEditor;
    assert.ok(active);
    assert.strictEqual(active.document.uri.toString(), editor.document.uri.toString());
    assert.strictEqual(active.document.getText(active.selection), 'c1');
  });

  test('editing the buffer after a solve marks the solution stale', async () => {
    await vscode.window.showTextDocument(editor.document);
    events.length = 0;
    await vscode.commands.executeCommand('lp.solve');
    await waitFor('solution data', () => events.find((e) => e.view === 'solution' && e.direction === 'out' && e.type === 'data'), 60_000);
    assert.ok(
      !events.some((e) => e.view === 'solution' && e.direction === 'out' && e.type === 'data' && e.stale === true),
      'a fresh solve is not stale',
    );
    const before = events.length;
    // The solution panel is the active editor now, so edit the document directly.
    const change = new vscode.WorkspaceEdit();
    change.insert(editor.document.uri, new vscode.Position(0, 0), '\\ edited\n');
    const edited = await vscode.workspace.applyEdit(change);
    assert.ok(edited, 'the edit applied');
    await waitFor('stale solution data', () =>
      events.slice(before).find((e) => e.view === 'solution' && e.direction === 'out' && e.type === 'data' && e.stale === true),
    );
    await vscode.window.showTextDocument(editor.document);
    await vscode.commands.executeCommand('workbench.action.files.revert');
  });

  test('lp.solve with a document URI solves that document even when another editor is active', async () => {
    const other = await vscode.workspace.openTextDocument({ language: 'plaintext', content: 'not a model' });
    await vscode.window.showTextDocument(other);
    events.length = 0;
    await vscode.commands.executeCommand('lp.solve', editor.document.uri.toString());
    await waitFor('solution data', () => events.find((e) => e.view === 'solution' && e.direction === 'out' && e.type === 'data'), 60_000);
  });

  test('lp.explainConstraint with a URI and position explains that constraint without a quick pick', async () => {
    // The cursor is on line 0, which the stand-in cannot explain: only the arguments can find `c1`.
    editor = await vscode.window.showTextDocument(editor.document);
    editor.selection = new vscode.Selection(0, 0, 0, 0);
    events.length = 0;
    await vscode.commands.executeCommand('lp.explainConstraint', editor.document.uri.toString(), { line: 3, character: 1 });
    // The panel from the earlier test is reused, so only the data is posted again.
    await waitFor('explain data', () => events.find((e) => e.view === 'explain' && e.direction === 'out' && e.type === 'data'));
  });
});
