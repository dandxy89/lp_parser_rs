import * as vscode from 'vscode';
import { WebviewMessage } from '../pure/messages';
import { ViewName } from '../pure/pages';
import { EntityKind, parseExplainResult } from '../pure/protocol';
import { describeRequestError, isCancellation } from '../pure/requestErrors';
import { LpPanel } from './panel';
import { isStale, lpRequest, revealRange } from './lpClient';

/** Re-run a view this long after the last edit. */
const REFRESH_DELAY_MS = 800;

export interface ViewEntry<T, S> {
  readonly panel: LpPanel;
  readonly uri: vscode.Uri;
  result: T | undefined;
  state: S;
  source: vscode.CancellationTokenSource | undefined;
  timer: NodeJS.Timeout | undefined;
}

export interface ViewSpec<T, S> {
  readonly view: ViewName;
  readonly title: (document: vscode.TextDocument) => string;
  readonly initialState: () => S;
  /** Fetches and parses; throws when the server answered with something unusable. */
  readonly fetch: (entry: ViewEntry<T, S>, document: vscode.TextDocument, token: vscode.CancellationToken) => Promise<T>;
  readonly versionOf: (result: T) => number | null;
  /** View-specific messages (`window`, `focus`, `revealItem`, ...). */
  readonly handle?: (entry: ViewEntry<T, S>, message: WebviewMessage, reload: () => Promise<void>) => Promise<void>;
  readonly autoRefresh: boolean;
}

/** One live webview per document and view kind: loads on open, refreshes after edits, resolves clicks to source. */
export class DocumentView<T, S> implements vscode.Disposable {
  private readonly entries = new Map<string, ViewEntry<T, S>>();
  private readonly subscription: vscode.Disposable;

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly spec: ViewSpec<T, S>,
  ) {
    this.subscription = vscode.workspace.onDidChangeTextDocument((event) => {
      const entry = this.entries.get(event.document.uri.toString());
      if (entry !== undefined && !this.spec.autoRefresh && entry.result !== undefined && event.contentChanges.length > 0) {
        // Not refreshed automatically (a solve is costly): flag the shown result as out of date.
        this.show(entry, entry.result, event.document);
      }
      if (entry !== undefined && this.spec.autoRefresh && event.contentChanges.length > 0) {
        if (entry.timer !== undefined) {
          clearTimeout(entry.timer);
        }
        entry.timer = setTimeout(() => void this.load(entry), REFRESH_DELAY_MS);
      }
    });
  }

  /** Opens (or focuses) the view for `document`. */
  async open(document: vscode.TextDocument, initial?: (state: S) => void): Promise<ViewEntry<T, S>> {
    const key = document.uri.toString();
    const existing = this.entries.get(key);
    if (existing !== undefined && !existing.panel.isDisposed) {
      initial?.(existing.state);
      existing.panel.reveal();
      await this.load(existing);
      return existing;
    }
    const state = this.spec.initialState();
    initial?.(state);
    // The entry must exist before the panel can post `ready`, so build it with a late-bound panel.
    const holder: { entry?: ViewEntry<T, S> } = {};
    const panel = new LpPanel({
      context: this.context,
      view: this.spec.view,
      title: this.spec.title(document),
      onMessage: (message) => this.onMessage(holder.entry, message),
      onDispose: () => this.disposeEntry(key),
    });
    const entry: ViewEntry<T, S> = { panel, uri: document.uri, result: undefined, state, source: undefined, timer: undefined };
    holder.entry = entry;
    this.entries.set(key, entry);
    // The first load happens on the webview's `ready` message, once its script is listening.
    return entry;
  }

  /** Posts a result for an entry (used by views whose data does not come from `fetch`, such as solve). */
  show(entry: ViewEntry<T, S>, result: T, document: vscode.TextDocument): void {
    entry.result = result;
    entry.panel.data(result, { stale: isStale(this.spec.versionOf(result), document) });
  }

  async load(entry: ViewEntry<T, S>): Promise<void> {
    entry.source?.cancel();
    entry.source?.dispose();
    const source = new vscode.CancellationTokenSource();
    entry.source = source;
    entry.panel.busy(true);
    try {
      const document = await vscode.workspace.openTextDocument(entry.uri);
      const result = await this.spec.fetch(entry, document, source.token);
      if (source.token.isCancellationRequested) {
        return;
      }
      this.show(entry, result, document);
    } catch (error) {
      if (!isCancellation(error) && !source.token.isCancellationRequested) {
        entry.panel.error(error instanceof Error && !('code' in error) ? error.message : describeRequestError(error));
      }
    } finally {
      if (entry.source === source) {
        entry.panel.busy(false);
      }
    }
  }

  private async onMessage(entry: ViewEntry<T, S> | undefined, message: WebviewMessage): Promise<void> {
    if (entry === undefined) {
      return;
    }
    switch (message.type) {
      case 'ready':
      case 'refresh':
        await this.load(entry);
        return;
      case 'reveal':
        await revealEntity(entry.uri, message.kind, message.name);
        return;
      case 'explain':
        await vscode.commands.executeCommand('lp.explainEntity', entry.uri.toString(), message.kind, message.name);
        return;
      default:
        await this.spec.handle?.(entry, message, () => this.load(entry));
    }
  }

  private disposeEntry(key: string): void {
    const entry = this.entries.get(key);
    if (entry !== undefined) {
      entry.source?.cancel();
      entry.source?.dispose();
      if (entry.timer !== undefined) {
        clearTimeout(entry.timer);
      }
      this.entries.delete(key);
    }
  }

  dispose(): void {
    this.subscription.dispose();
    for (const entry of [...this.entries.values()]) {
      entry.panel.dispose();
    }
  }
}

/** Resolves a name to its definition with `lp/explain` and opens it. */
export async function revealEntity(uri: vscode.Uri, kind: EntityKind, name: string): Promise<void> {
  try {
    const raw = await lpRequest('lp/explain', { textDocument: { uri: uri.toString() }, target: { kind, name } });
    const result = parseExplainResult(raw);
    if (result?.range === undefined) {
      void vscode.window.showInformationMessage(`LP: no source location for ${kind} '${name}'.`);
      return;
    }
    await revealRange(uri, result.range);
  } catch (error) {
    void vscode.window.showWarningMessage(`LP: ${describeRequestError(error)}`);
  }
}
