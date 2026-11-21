import * as vscode from 'vscode';
import { MAX_MPS_BYTES, parseConvertFromMpsResult } from './pure/mps';

/** Arguments for `lp.convertFromMps`: the active MPS document's URI and text (MPS buffers are not synchronised with the server). */
export function activeMpsArgs(): unknown[] | undefined {
  const editor = vscode.window.activeTextEditor;
  if (editor === undefined || editor.document.languageId !== 'mps') {
    void vscode.window.showWarningMessage('Open an MPS file to run this command.');
    return undefined;
  }
  const text = editor.document.getText();
  if (Buffer.byteLength(text, 'utf8') > MAX_MPS_BYTES) {
    void vscode.window.showErrorMessage(`This MPS file is larger than the ${MAX_MPS_BYTES / (1024 * 1024)} MiB conversion limit.`);
    return undefined;
  }
  return [editor.document.uri.toString(), text];
}

/** Opens the converted LP text in a new unsaved editor; the server has already reported any omitted rows. */
export async function openConvertedLp(result: unknown, log: (message: string) => void): Promise<void> {
  const converted = parseConvertFromMpsResult(result);
  if (converted === undefined) {
    log('lp.convertFromMps: unexpected result');
    return;
  }
  const document = await vscode.workspace.openTextDocument({ language: 'lp', content: converted.lp });
  await vscode.window.showTextDocument(document, { preview: false });
}
