import * as vscode from 'vscode';
import type { LanguageClient } from 'vscode-languageclient/node';
import type { LpRequest } from '../pure/capabilities';
import { LspRange } from '../pure/sanitise';

let provider: () => LanguageClient | undefined = () => undefined;

/** The extension gives the views access to the current client (it is recreated on restart). */
export function setClientProvider(next: () => LanguageClient | undefined): void {
  provider = next;
}

/** Sends an `lp/*` request. Rejects with the client's `ResponseError` (see `describeRequestError`). */
export async function lpRequest(method: LpRequest, params: object, token?: vscode.CancellationToken): Promise<unknown> {
  const client = provider();
  if (client === undefined) {
    throw new Error('The LP language server is not running.');
  }
  return token === undefined ? client.sendRequest(method, params) : client.sendRequest(method, params, token);
}

/** The active LP document, or a warning and `undefined`. */
export function activeLpDocument(): vscode.TextDocument | undefined {
  const document = vscode.window.activeTextEditor?.document;
  if (document === undefined || document.languageId !== 'lp') {
    void vscode.window.showWarningMessage('Open an LP file to run this command.');
    return undefined;
  }
  return document;
}

/** `true` when a result computed for `version` no longer describes `document`. */
export function isStale(version: number | null, document: vscode.TextDocument): boolean {
  return version !== null && version < document.version;
}

/** Opens `uri` and selects `range` (LSP positions, which VS Code shares in zero-based lines). */
export async function revealRange(uri: vscode.Uri, range: LspRange): Promise<void> {
  const target = new vscode.Range(range.start.line, range.start.character, range.end.line, range.end.character);
  const editor = await vscode.window.showTextDocument(uri, { selection: target, preserveFocus: false, preview: false });
  editor.revealRange(target, vscode.TextEditorRevealType.InCenterIfOutsideViewport);
}
