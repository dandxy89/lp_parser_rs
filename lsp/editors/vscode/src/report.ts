import * as path from 'node:path';
import * as vscode from 'vscode';
import { collectDiagnostics } from './modelViews';
import type { ModelClient } from './pure/modelTypes';
import { describeRequestError } from './pure/requestErrors';
import { renderModelReport, reportFileName } from './pure/report';
import { activeLpDocument } from './views/lpClient';

function defaultTarget(document: vscode.TextDocument): vscode.Uri | undefined {
  if (document.uri.scheme === 'file') {
    return vscode.Uri.file(path.join(path.dirname(document.uri.fsPath), reportFileName(path.basename(document.uri.fsPath))));
  }
  const folder = vscode.workspace.workspaceFolders?.[0];
  return folder === undefined ? undefined : vscode.Uri.joinPath(folder.uri, 'model.report.md');
}

async function exportReport(client: ModelClient, log: (message: string) => void): Promise<void> {
  const document = activeLpDocument();
  if (document === undefined) {
    return;
  }
  try {
    const model = await client.model(document.uri.toString());
    const markdown = renderModelReport({
      fileName: path.basename(document.uri.path),
      model,
      diagnostics: collectDiagnostics(document.uri),
    });
    const target = await vscode.window.showSaveDialog({
      defaultUri: defaultTarget(document),
      filters: { Markdown: ['md'] },
      saveLabel: 'Export Report',
    });
    if (target === undefined) {
      return;
    }
    await vscode.workspace.fs.writeFile(target, Buffer.from(markdown, 'utf8'));
    try {
      await vscode.commands.executeCommand('markdown.showPreview', target);
    } catch (error) {
      log(`lp.exportReport: markdown preview unavailable (${String(error)}); showing the source`);
      await vscode.window.showTextDocument(target);
    }
  } catch (error) {
    void vscode.window.showErrorMessage(`Could not export the model report: ${describeRequestError(error)}`);
  }
}

/** Registers `lp.exportReport`. The command ignores its arguments, so a command link cannot choose where it writes. */
export function registerReport(context: vscode.ExtensionContext, client: ModelClient, log: (message: string) => void): void {
  context.subscriptions.push(vscode.commands.registerCommand('lp.exportReport', () => exportReport(client, log)));
}
