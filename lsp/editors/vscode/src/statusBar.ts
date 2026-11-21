import * as vscode from 'vscode';
import { DiagnosticCounts, ModelStats, ServerState, formatStatusText, formatStatusTooltip } from './pure/status';

/** Status bar item showing the server state and, for the active LP document, its diagnostic counts. */
export class LpStatusBar implements vscode.Disposable {
  private readonly item: vscode.StatusBarItem;
  private readonly disposables: vscode.Disposable[];
  private state: ServerState = 'starting';
  private stats: ModelStats | undefined;

  constructor() {
    this.item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
    this.item.name = 'LP language server';
    this.item.command = 'lp.analyze';
    this.disposables = [
      this.item,
      vscode.languages.onDidChangeDiagnostics(() => this.refresh()),
      vscode.window.onDidChangeActiveTextEditor(() => this.refresh()),
    ];
    this.refresh();
    this.item.show();
  }

  setState(state: ServerState): void {
    this.state = state;
    this.refresh();
  }

  /** Sets the model size shown for the active LP document; `undefined` hides it. */
  setStats(stats: ModelStats | undefined): void {
    this.stats = stats;
    this.refresh();
  }

  refresh(): void {
    const editor = vscode.window.activeTextEditor;
    const counts = editor?.document.languageId === 'lp' ? countDiagnostics(editor.document.uri) : undefined;
    const stats = counts === undefined ? undefined : this.stats;
    this.item.text = formatStatusText(this.state, counts, stats);
    this.item.tooltip = formatStatusTooltip(this.state, counts, stats);
  }

  dispose(): void {
    for (const disposable of this.disposables) {
      disposable.dispose();
    }
  }
}

function countDiagnostics(uri: vscode.Uri): DiagnosticCounts {
  let errors = 0;
  let warnings = 0;
  for (const diagnostic of vscode.languages.getDiagnostics(uri)) {
    if (diagnostic.severity === vscode.DiagnosticSeverity.Error) {
      errors += 1;
    } else if (diagnostic.severity === vscode.DiagnosticSeverity.Warning) {
      warnings += 1;
    }
  }
  return { errors, warnings };
}
