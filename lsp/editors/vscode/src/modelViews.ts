import * as vscode from 'vscode';
import { AnalysisDiagnostic, AnalysisNode, groupByCategory, readDiagnosticData } from './pure/analysis';
import { ExplorerNode, ModelExplorer } from './pure/explorer';
import type { LspRange, ModelClient } from './pure/modelTypes';
import type { LpStatusBar } from './statusBar';

const REFRESH_DELAY_MS = 300;

function toRange(range: LspRange): vscode.Range {
  return new vscode.Range(range.start.line, range.start.character, range.end.line, range.end.character);
}

function activeLpUri(): vscode.Uri | undefined {
  const editor = vscode.window.activeTextEditor;
  return editor?.document.languageId === 'lp' ? editor.document.uri : undefined;
}

function icon(id: string): vscode.ThemeIcon {
  return new vscode.ThemeIcon(id);
}

/** Opens the document at `uri` and reveals `range`; the click target of every explorer and analysis leaf. */
async function revealRange(uri: string, range: LspRange): Promise<void> {
  const target = toRange(range);
  await vscode.window.showTextDocument(vscode.Uri.parse(uri), { selection: target, preserveFocus: false });
}

class ExplorerProvider implements vscode.TreeDataProvider<ExplorerNode> {
  private readonly emitter = new vscode.EventEmitter<ExplorerNode | undefined>();
  readonly onDidChangeTreeData = this.emitter.event;

  constructor(private readonly explorer: ModelExplorer) {}

  refresh(): void {
    this.emitter.fire(undefined);
  }

  getTreeItem(node: ExplorerNode): vscode.TreeItem {
    const item = new vscode.TreeItem(
      node.label,
      node.expandable ? vscode.TreeItemCollapsibleState.Collapsed : vscode.TreeItemCollapsibleState.None,
    );
    item.id = node.id;
    item.description = node.description;
    item.tooltip = node.tooltip;
    item.iconPath = icon(node.icon);
    item.contextValue = node.kind;
    const uri = this.explorer.documentUri;
    if (node.range !== undefined && uri !== undefined) {
      item.command = { command: 'lp.revealRange', title: 'Go to source', arguments: [uri, node.range] };
    }
    return item;
  }

  getChildren(node?: ExplorerNode): Promise<ExplorerNode[]> {
    return node === undefined ? this.explorer.roots() : this.explorer.children(node);
  }
}

class AnalysisProvider implements vscode.TreeDataProvider<AnalysisNode> {
  private readonly emitter = new vscode.EventEmitter<AnalysisNode | undefined>();
  readonly onDidChangeTreeData = this.emitter.event;
  private tree: readonly AnalysisNode[] = [];
  private uri: string | undefined;

  refresh(): void {
    const uri = activeLpUri();
    this.uri = uri?.toString();
    this.tree = uri === undefined ? [] : groupByCategory(collectDiagnostics(uri));
    this.emitter.fire(undefined);
  }

  getTreeItem(node: AnalysisNode): vscode.TreeItem {
    const item = new vscode.TreeItem(
      node.label,
      node.children.length > 0 ? vscode.TreeItemCollapsibleState.Collapsed : vscode.TreeItemCollapsibleState.None,
    );
    item.id = node.id;
    item.description = node.description;
    item.iconPath = icon(node.icon);
    item.contextValue = node.kind;
    if (node.range !== undefined && this.uri !== undefined) {
      item.command = { command: 'lp.revealRange', title: 'Go to source', arguments: [this.uri, node.range] };
    }
    return item;
  }

  getChildren(node?: AnalysisNode): AnalysisNode[] {
    return [...(node === undefined ? this.tree : node.children)];
  }
}

export function collectDiagnostics(uri: vscode.Uri): AnalysisDiagnostic[] {
  const out: AnalysisDiagnostic[] = [];
  for (const diagnostic of vscode.languages.getDiagnostics(uri)) {
    // Only server findings carry a code; syntax errors and other sources are not analysis findings.
    const code = typeof diagnostic.code === 'object' ? String(diagnostic.code.value) : diagnostic.code;
    if (code === undefined || code === '') {
      continue;
    }
    const { category, certainty } = readDiagnosticData((diagnostic as { data?: unknown }).data);
    out.push({
      code: String(code),
      message: diagnostic.message,
      // vscode severities run 0 (error) to 3 (hint); LSP's run 1 to 4.
      severity: (diagnostic.severity + 1) as 1 | 2 | 3 | 4,
      category,
      certainty,
      range: {
        start: { line: diagnostic.range.start.line, character: diagnostic.range.start.character },
        end: { line: diagnostic.range.end.line, character: diagnostic.range.end.character },
      },
    });
  }
  return out;
}

/** What `registerModelViews` hands back: a refresh for server (re)starts, and read access to the tree data for tests. */
export interface ModelViews {
  readonly refresh: () => void;
  /** The Model Explorer's root nodes for the active document. */
  readonly explorerRoots: () => Promise<ExplorerNode[]>;
  /** The Analysis view's current top-level nodes. */
  readonly analysisTree: () => AnalysisNode[];
}

/**
 * Registers the Model Explorer and Analysis views and keeps them, and the status bar's model stats, in step with
 * the active LP document. Returns a refresh to call when the server (re)starts.
 */
export function registerModelViews(
  context: vscode.ExtensionContext,
  client: ModelClient,
  statusBar: LpStatusBar,
  log: (message: string) => void,
): ModelViews {
  const explorer = new ModelExplorer(client);
  const explorerProvider = new ExplorerProvider(explorer);
  const analysisProvider = new AnalysisProvider();
  const explorerView = vscode.window.createTreeView('lp.modelExplorer', { treeDataProvider: explorerProvider, showCollapseAll: true });
  let statsGeneration = 0;

  const updateStats = async (uri: vscode.Uri | undefined): Promise<void> => {
    const generation = ++statsGeneration;
    if (uri === undefined) {
      statusBar.setStats(undefined);
      return;
    }
    try {
      const result = await client.model(uri.toString(), ['summary']);
      if (generation === statsGeneration && result.summary !== undefined) {
        statusBar.setStats({
          objectives: result.summary.objectives,
          constraints: result.summary.constraints,
          variables: result.summary.variables,
        });
      }
    } catch (error) {
      // The server may be starting or lack lp/model; the status bar falls back to counts only.
      log(`lp/model summary failed: ${error instanceof Error ? error.message : String(error)}`);
      if (generation === statsGeneration) {
        statusBar.setStats(undefined);
      }
    }
  };

  const syncFilterDescription = (): void => {
    explorerView.description = explorer.filterText.length > 0 ? `filter: ${explorer.filterText}` : undefined;
  };

  const refreshNow = (): void => {
    const uri = activeLpUri();
    explorer.setDocument(uri?.toString());
    explorer.invalidate();
    explorerProvider.refresh();
    analysisProvider.refresh();
    void updateStats(uri);
  };

  let timer: NodeJS.Timeout | undefined;
  const schedule = (): void => {
    if (timer !== undefined) {
      clearTimeout(timer);
    }
    timer = setTimeout(refreshNow, REFRESH_DELAY_MS);
  };

  context.subscriptions.push(
    explorerView,
    vscode.window.createTreeView('lp.analysis', { treeDataProvider: analysisProvider, showCollapseAll: true }),
    { dispose: () => timer !== undefined && clearTimeout(timer) },
    vscode.window.onDidChangeActiveTextEditor(schedule),
    // Diagnostics are republished after each analysis, so this also catches edits.
    vscode.languages.onDidChangeDiagnostics((event) => {
      const uri = activeLpUri();
      if (uri !== undefined && event.uris.some((changed) => changed.toString() === uri.toString())) {
        schedule();
      }
    }),
    vscode.commands.registerCommand('lp.revealRange', revealRange),
    vscode.commands.registerCommand('lp.showModelExplorer', () => vscode.commands.executeCommand('lp.modelExplorer.focus')),
    vscode.commands.registerCommand('lp.explorer.refresh', refreshNow),
    vscode.commands.registerCommand('lp.explorer.filter', async () => {
      const text = await vscode.window.showInputBox({
        prompt: 'Filter the Model Explorer by name',
        value: explorer.filterText,
        placeHolder: 'e.g. capacity',
      });
      if (text !== undefined) {
        explorer.setFilter(text);
        syncFilterDescription();
        explorerProvider.refresh();
      }
    }),
    vscode.commands.registerCommand('lp.explorer.clearFilter', () => {
      explorer.setFilter('');
      syncFilterDescription();
      explorerProvider.refresh();
    }),
  );

  refreshNow();
  return {
    refresh: refreshNow,
    explorerRoots: () => explorerProvider.getChildren(),
    analysisTree: () => analysisProvider.getChildren(),
  };
}
