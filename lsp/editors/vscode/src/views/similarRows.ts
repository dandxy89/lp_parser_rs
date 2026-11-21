import * as vscode from 'vscode';
import { parseExplainResult } from '../pure/protocol';
import { LspRange } from '../pure/sanitise';
import { SimilarDiagnostic, findingsFor, similarFindings, similarPicks } from '../pure/similarRows';
import { lpRequest, revealRange } from './lpClient';

function toLspRange(range: vscode.Range): LspRange {
  return { start: { line: range.start.line, character: range.start.character }, end: { line: range.end.line, character: range.end.character } };
}

function toSimilarDiagnostic(document: vscode.TextDocument, diagnostic: vscode.Diagnostic): SimilarDiagnostic {
  const code = typeof diagnostic.code === 'object' ? String(diagnostic.code.value) : diagnostic.code === undefined ? undefined : String(diagnostic.code);
  const uri = document.uri.toString();
  return {
    source: diagnostic.source,
    code,
    message: diagnostic.message,
    range: toLspRange(diagnostic.range),
    related: (diagnostic.relatedInformation ?? []).filter((r) => r.location.uri.toString() === uri).map((r) => toLspRange(r.location.range)),
  };
}

/** The constraint under the cursor, if the server can explain one there. */
async function constraintAtCursor(document: vscode.TextDocument): Promise<{ name: string; range: LspRange } | undefined> {
  const editor = vscode.window.activeTextEditor;
  if (editor === undefined || editor.document !== document) {
    return undefined;
  }
  try {
    const raw = await lpRequest('lp/explain', { textDocument: { uri: document.uri.toString() }, position: editor.selection.active });
    const result = parseExplainResult(raw);
    return result?.kind === 'constraint' && result.range !== undefined ? { name: result.name, range: result.range } : undefined;
  } catch {
    // Nothing explainable at the cursor, or the request is unavailable: list the whole model instead.
    return undefined;
  }
}

/**
 * Lists the duplicate and proportional rows the server already reported, for the constraint under the cursor or else
 * the whole model. The result reflects the last published analysis, not keystrokes still being debounced.
 */
export async function findSimilarConstraints(document: vscode.TextDocument): Promise<void> {
  const file = document.uri.path.split('/').pop() ?? document.uri.path;
  const { findings, notes } = similarFindings(vscode.languages.getDiagnostics(document.uri).map((d) => toSimilarDiagnostic(document, d)));
  const focus = await constraintAtCursor(document);
  const scoped = focus === undefined ? [] : findingsFor(findings, focus.range);
  const shown = scoped.length > 0 ? scoped : findings;
  if (shown.length === 0 && notes.length === 0) {
    void vscode.window.showInformationMessage(`LP: no duplicate or proportional rows are reported for ${file}. lp.analysis settings can hide them.`);
    return;
  }
  const nameAt = (range: LspRange): string =>
    document.getText(new vscode.Range(range.start.line, range.start.character, range.end.line, range.end.character));
  const picks = similarPicks(shown, nameAt, scoped.length > 0 ? focus?.range : undefined);
  const items: (vscode.QuickPickItem & { target?: LspRange })[] = picks.map((p) => ({ label: p.label, description: p.description, detail: p.detail, target: p.target }));
  if (notes.length > 0) {
    items.push({ label: '', kind: vscode.QuickPickItemKind.Separator });
    items.push(...notes.map((message) => ({ label: `$(info) ${message}` })));
  }
  const placeHolder =
    focus === undefined
      ? `Similar rows in ${file}`
      : scoped.length > 0
        ? `Rows similar to ${focus.name}`
        : `No rows similar to ${focus.name}; every similar row in ${file}`;
  const pick = await vscode.window.showQuickPick(items, { placeHolder, matchOnDescription: true, matchOnDetail: true });
  if (pick?.target !== undefined) {
    await revealRange(document.uri, pick.target);
  }
}
