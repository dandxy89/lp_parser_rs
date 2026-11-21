import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import * as vscode from 'vscode';
import {
  DiffResult,
  EntityKind,
  ExplainResult,
  GraphResult,
  MatrixResult,
  SolveResult,
  parseDiffResult,
  parseExplainResult,
  parseGraphResult,
  parseMatrixResult,
  parseModelNames,
  parseSolveResult,
} from '../pure/protocol';
import { MatrixMode, DETAIL_WINDOW, defaultMode } from '../pure/matrixView';
import { diffRange, explainRange, solutionRange } from '../pure/ranges';
import { describeRequestError, isCancellation } from '../pure/requestErrors';
import { DocumentView } from './documentView';
import { activeLpDocument, isStale, lpRequest, revealRange } from './lpClient';

/** Largest comparison text the extension will send (the server has its own limit). */
const MAX_DIFF_BYTES = 64 * 1024 * 1024;

function textDocument(document: vscode.TextDocument): { uri: string } {
  return { uri: document.uri.toString() };
}

function fileLabel(uri: vscode.Uri): string {
  return path.basename(uri.fsPath);
}

// ------------------------------------------------------------------ matrix

interface MatrixState {
  mode: MatrixMode | undefined;
  rowOffset: number;
  colOffset: number;
}

function createMatrixView(context: vscode.ExtensionContext): DocumentView<MatrixResult, MatrixState> {
  return new DocumentView<MatrixResult, MatrixState>(context, {
    view: 'matrix',
    title: (document) => `Matrix: ${fileLabel(document.uri)}`,
    initialState: () => ({ mode: undefined, rowOffset: 0, colOffset: 0 }),
    autoRefresh: true,
    versionOf: (result) => result.version,
    fetch: async (entry, document, token) => {
      const request = async (mode: MatrixMode): Promise<MatrixResult> => {
        const params =
          mode === 'entries'
            ? {
                textDocument: textDocument(document),
                mode,
                rows: { offset: entry.state.rowOffset, limit: DETAIL_WINDOW },
                cols: { offset: entry.state.colOffset, limit: DETAIL_WINDOW },
              }
            : { textDocument: textDocument(document), mode, bins: { rows: 128, cols: 128 } };
        const parsed = parseMatrixResult(await lpRequest('lp/matrix', params, token));
        if (parsed === undefined) {
          throw new Error('The server returned an unreadable matrix.');
        }
        return parsed;
      };
      let result = await request(entry.state.mode ?? 'entries');
      if (entry.state.mode === undefined) {
        entry.state.mode = defaultMode(result.shape);
        if (entry.state.mode !== result.mode) {
          result = await request(entry.state.mode);
        }
      }
      return result;
    },
    handle: async (entry, message, reload) => {
      if (message.type === 'window') {
        entry.state.rowOffset = message.rowOffset;
        entry.state.colOffset = message.colOffset;
        entry.state.mode = 'entries';
        await reload();
      } else if (message.type === 'mode') {
        entry.state.mode = message.mode;
        await reload();
      }
    },
  });
}

// ------------------------------------------------------------------ graph

interface GraphState {
  focus: { kind: EntityKind; name: string; depth: 1 | 2 | 3 } | undefined;
}

function createGraphView(context: vscode.ExtensionContext): DocumentView<GraphResult, GraphState> {
  return new DocumentView<GraphResult, GraphState>(context, {
    view: 'graph',
    title: (document) => `Graph: ${fileLabel(document.uri)}`,
    initialState: () => ({ focus: undefined }),
    autoRefresh: true,
    versionOf: (result) => result.version,
    fetch: async (entry, document, token) => {
      const params = {
        textDocument: textDocument(document),
        includeObjectives: true,
        maxNodes: 2000,
        maxEdges: 20000,
        ...(entry.state.focus !== undefined ? { focus: entry.state.focus } : {}),
      };
      const parsed = parseGraphResult(await lpRequest('lp/graph', params, token));
      if (parsed === undefined) {
        throw new Error('The server returned an unreadable graph.');
      }
      return parsed;
    },
    handle: async (entry, message, reload) => {
      if (message.type === 'focus') {
        entry.state.focus = { kind: message.kind, name: message.name, depth: message.depth };
        await reload();
      } else if (message.type === 'clearFocus') {
        entry.state.focus = undefined;
        await reload();
      }
    },
  });
}

// ------------------------------------------------------------------ diff

type DiffBase = { readonly text: string; readonly label: string } | { readonly uri: string; readonly label: string };

interface DiffState {
  base: DiffBase | undefined;
}

function createDiffView(context: vscode.ExtensionContext): DocumentView<DiffResult, DiffState> {
  return new DocumentView<DiffResult, DiffState>(context, {
    view: 'diff',
    title: (document) => `Diff: ${fileLabel(document.uri)}`,
    initialState: () => ({ base: undefined }),
    autoRefresh: true,
    versionOf: (result) => result.version,
    fetch: async (entry, document, token) => {
      const base = entry.state.base;
      if (base === undefined) {
        throw new Error('Choose what to compare with.');
      }
      const against = 'text' in base ? { text: base.text, label: base.label } : { uri: base.uri, label: base.label };
      const parsed = parseDiffResult(await lpRequest('lp/diff', { textDocument: textDocument(document), against }, token));
      if (parsed === undefined) {
        throw new Error('The server returned an unreadable diff.');
      }
      return parsed;
    },
    handle: async (entry, message) => {
      if (message.type === 'revealItem' && entry.result !== undefined) {
        const range = diffRange(entry.result, message.group, message.index);
        if (range !== undefined) {
          await revealRange(entry.uri, range);
        }
      }
    },
  });
}

interface GitRepository {
  show(ref: string, path: string): Promise<string>;
}

interface GitApi {
  getRepository(uri: vscode.Uri): GitRepository | null;
}

/** The text of `uri` at git HEAD through VS Code's built-in git extension. */
async function headText(uri: vscode.Uri): Promise<string> {
  const extension = vscode.extensions.getExtension<{ getAPI(version: 1): GitApi }>('vscode.git');
  if (extension === undefined) {
    throw new Error('The built-in Git extension is disabled.');
  }
  const git = (extension.isActive ? extension.exports : await extension.activate()).getAPI(1);
  const repository = git.getRepository(uri);
  if (repository === null) {
    throw new Error('This file is not in a Git repository.');
  }
  return repository.show('HEAD', uri.fsPath);
}

function checkSize(text: string, label: string): string {
  if (Buffer.byteLength(text, 'utf8') > MAX_DIFF_BYTES) {
    throw new Error(`${label} is larger than ${MAX_DIFF_BYTES / (1024 * 1024)} MiB.`);
  }
  return text;
}

async function chooseBase(document: vscode.TextDocument): Promise<DiffBase | undefined> {
  const pick = await vscode.window.showQuickPick(
    [
      { label: 'Git HEAD', description: 'The last commit', id: 'head' },
      { label: 'Last saved version', description: document.isDirty ? 'The file on disk, without unsaved edits' : 'The file on disk', id: 'saved' },
      { label: 'Another file…', description: 'Compare with a model on disk', id: 'file' },
    ],
    { placeHolder: 'Compare this model with' },
  );
  if (pick === undefined) {
    return undefined;
  }
  if (pick.id === 'head') {
    return { text: checkSize(await headText(document.uri), 'The HEAD version'), label: 'HEAD' };
  }
  if (pick.id === 'saved') {
    if (document.isUntitled) {
      throw new Error('An untitled model has no saved version.');
    }
    return { text: checkSize(await fs.readFile(document.uri.fsPath, 'utf8'), 'The saved file'), label: 'saved' };
  }
  const [other] =
    (await vscode.window.showOpenDialog({ canSelectMany: false, filters: { 'LP models': ['lp', 'LP', 'Lp'] }, openLabel: 'Compare' })) ?? [];
  return other === undefined ? undefined : { uri: other.toString(), label: fileLabel(other) };
}

// ------------------------------------------------------------------ solution

interface SolutionState {
  diagnose: boolean;
}

function createSolutionView(context: vscode.ExtensionContext): DocumentView<SolveResult, SolutionState> {
  return new DocumentView<SolveResult, SolutionState>(context, {
    view: 'solution',
    title: (document) => `Solution: ${fileLabel(document.uri)}`,
    initialState: () => ({ diagnose: false }),
    autoRefresh: false,
    versionOf: (result) => result.version,
    fetch: async (_entry, document, token) =>
      vscode.window.withProgress(
        { location: vscode.ProgressLocation.Notification, title: `Solving ${fileLabel(document.uri)}`, cancellable: true },
        async (_progress, userToken) => {
          const source = new vscode.CancellationTokenSource();
          const subscriptions = [userToken.onCancellationRequested(() => source.cancel()), token.onCancellationRequested(() => source.cancel())];
          try {
            const raw = await lpRequest('lp/solve', { textDocument: textDocument(document), diagnose: 'auto' }, source.token);
            const parsed = parseSolveResult(raw);
            if (parsed === undefined) {
              throw new Error('The server returned an unreadable solution.');
            }
            return parsed;
          } finally {
            subscriptions.forEach((subscription) => subscription.dispose());
            source.dispose();
          }
        },
      ),
    handle: async (entry, message) => {
      if (message.type === 'revealItem' && entry.result !== undefined) {
        const document = await vscode.workspace.openTextDocument(entry.uri);
        if (isStale(entry.result.version, document)) {
          // The ranges belong to an older version of the document and would point at the wrong lines.
          void vscode.window.showInformationMessage('LP: the document changed since this solve; solve again to navigate.');
          return;
        }
        const range = solutionRange(entry.result, message.group, message.index);
        if (range !== undefined) {
          await revealRange(entry.uri, range);
        }
      }
    },
  });
}

// ------------------------------------------------------------------ explain

interface ExplainState {
  target: { kind: EntityKind; name: string } | undefined;
}

function createExplainView(context: vscode.ExtensionContext): DocumentView<ExplainResult, ExplainState> {
  return new DocumentView<ExplainResult, ExplainState>(context, {
    view: 'explain',
    title: (document) => `Explain: ${fileLabel(document.uri)}`,
    initialState: () => ({ target: undefined }),
    autoRefresh: true,
    versionOf: () => null,
    fetch: async (entry, document, token) => {
      if (entry.state.target === undefined) {
        throw new Error('Nothing to explain.');
      }
      const parsed = parseExplainResult(await lpRequest('lp/explain', { textDocument: textDocument(document), target: entry.state.target }, token));
      if (parsed === undefined) {
        throw new Error('The server returned an unreadable explanation.');
      }
      entry.panel.setTitle(`Explain: ${parsed.name}`);
      return parsed;
    },
    handle: async (entry, message) => {
      if (message.type === 'revealItem' && entry.result !== undefined) {
        const range = explainRange(entry.result, message.group, message.index);
        if (range !== undefined) {
          await revealRange(entry.uri, range);
        }
      }
    },
  });
}

/** Names to choose from: the entity under the cursor if the server can tell, else a quick pick over the model. */
async function pickEntity(document: vscode.TextDocument, kinds: readonly EntityKind[]): Promise<{ kind: EntityKind; name: string } | undefined> {
  const editor = vscode.window.activeTextEditor;
  if (editor !== undefined && editor.document === document) {
    try {
      const raw = await lpRequest('lp/explain', { textDocument: textDocument(document), position: editor.selection.active });
      const result = parseExplainResult(raw);
      if (result !== undefined && kinds.includes(result.kind)) {
        return { kind: result.kind, name: result.name };
      }
    } catch {
      // The cursor is on nothing explainable: fall back to the list.
    }
  }
  const include = kinds.includes('variable') ? ['variables'] : ['constraints', 'objectives'];
  const names = parseModelNames(await lpRequest('lp/model', { textDocument: textDocument(document), include, limit: 50000 }));
  if (names === undefined) {
    throw new Error('The server returned an unreadable model.');
  }
  const items: { label: string; description: string; entity: EntityKind }[] = [
    ...(kinds.includes('constraint') ? names.constraints.map((label) => ({ label, description: 'constraint', entity: 'constraint' as const })) : []),
    ...(kinds.includes('objective') ? names.objectives.map((label) => ({ label, description: 'objective', entity: 'objective' as const })) : []),
    ...(kinds.includes('variable') ? names.variables.map((label) => ({ label, description: 'variable', entity: 'variable' as const })) : []),
  ];
  const pick = await vscode.window.showQuickPick(items, { placeHolder: kinds.includes('variable') ? 'Variable to explain' : 'Constraint to explain', matchOnDescription: true });
  return pick === undefined ? undefined : { kind: pick.entity, name: pick.label };
}

// ------------------------------------------------------------------ registration

/** Runs `action`, showing any failure as a warning. Cancellation is silent. */
async function guarded(action: () => Promise<void>): Promise<void> {
  try {
    await action();
  } catch (error) {
    if (!isCancellation(error)) {
      void vscode.window.showWarningMessage(`LP: ${error instanceof Error && !('code' in error) ? error.message : describeRequestError(error)}`);
    }
  }
}

/** Registers the webview commands. Their `enablement` in package.json hides them until the server advertises the request. */
export function registerViews(context: vscode.ExtensionContext): void {
  const matrix = createMatrixView(context);
  const graph = createGraphView(context);
  const diff = createDiffView(context);
  const solution = createSolutionView(context);
  const explain = createExplainView(context);
  context.subscriptions.push(matrix, graph, diff, solution, explain);

  const withDocument = (action: (document: vscode.TextDocument) => Promise<void>) => async (): Promise<void> => {
    const document = activeLpDocument();
    if (document !== undefined) {
      await guarded(() => action(document));
    }
  };
  const explainTarget = async (document: vscode.TextDocument, target: { kind: EntityKind; name: string } | undefined): Promise<void> => {
    if (target !== undefined) {
      await explain.open(document, (state) => {
        state.target = target;
      });
    }
  };

  context.subscriptions.push(
    vscode.commands.registerCommand('lp.showMatrix', withDocument(async (document) => void (await matrix.open(document)))),
    vscode.commands.registerCommand('lp.showDependencyGraph', withDocument(async (document) => void (await graph.open(document)))),
    vscode.commands.registerCommand(
      'lp.compareModels',
      withDocument(async (document) => {
        const base = await chooseBase(document);
        if (base !== undefined) {
          await diff.open(document, (state) => {
            state.base = base;
          });
        }
      }),
    ),
    vscode.commands.registerCommand('lp.solve', withDocument(async (document) => void (await solution.open(document)))),
    vscode.commands.registerCommand('lp.diagnoseInfeasibility', withDocument(async (document) => void (await solution.open(document)))),
    vscode.commands.registerCommand('lp.explainConstraint', withDocument(async (document) => explainTarget(document, await pickEntity(document, ['constraint', 'objective'])))),
    vscode.commands.registerCommand('lp.explainVariable', withDocument(async (document) => explainTarget(document, await pickEntity(document, ['variable'])))),
    // Internal: opened from links inside the explain panel and the other views.
    vscode.commands.registerCommand('lp.explainEntity', async (uri: unknown, kind: unknown, name: unknown) => {
      if (typeof uri !== 'string' || typeof name !== 'string' || (kind !== 'constraint' && kind !== 'objective' && kind !== 'variable')) {
        return;
      }
      const document = await vscode.workspace.openTextDocument(vscode.Uri.parse(uri));
      await guarded(() => explainTarget(document, { kind, name }));
    }),
  );
}
