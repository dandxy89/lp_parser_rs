import { randomBytes } from 'node:crypto';
import * as vscode from 'vscode';
import { WebviewMessage, parseWebviewMessage } from '../pure/messages';
import { ViewName, pageBody } from '../pure/pages';
import { WEBVIEW_CSS } from '../pure/webviewCss';
import { buildPage, makeNonce } from '../pure/webviewHtml';

/** One message crossing a panel boundary: `in` from the webview, `out` to it. */
export interface PanelEvent {
  readonly view: ViewName;
  readonly direction: 'in' | 'out';
  readonly type: string;
  /** On `data` events: whether the banner for an out-of-date result was requested. */
  readonly stale?: boolean;
}

let observer: ((event: PanelEvent) => void) | undefined;

/** Observes every panel message (the extension test host uses this); `undefined` stops observing. */
export function observePanels(next: ((event: PanelEvent) => void) | undefined): void {
  observer = next;
}

export interface PanelOptions {
  readonly context: vscode.ExtensionContext;
  readonly view: ViewName;
  readonly title: string;
  readonly column?: vscode.ViewColumn;
  readonly onMessage: (message: WebviewMessage) => void | Promise<void>;
  readonly onDispose: () => void;
}

/** One webview panel: strict CSP, a nonce per page, validated inbound messages. */
export class LpPanel implements vscode.Disposable {
  private readonly panel: vscode.WebviewPanel;
  private disposed = false;
  private readonly view: ViewName;

  constructor(options: PanelOptions) {
    this.view = options.view;
    const scripts = vscode.Uri.joinPath(options.context.extensionUri, 'dist', 'webviews');
    this.panel = vscode.window.createWebviewPanel(`lp.${options.view}`, options.title, options.column ?? vscode.ViewColumn.Beside, {
      enableScripts: true,
      retainContextWhenHidden: true,
      localResourceRoots: [scripts],
    });
    const nonce = makeNonce((n) => randomBytes(n));
    const scriptUri = this.panel.webview.asWebviewUri(vscode.Uri.joinPath(scripts, `${options.view}.js`)).toString();
    this.panel.webview.html = buildPage({ title: options.title, nonce, css: WEBVIEW_CSS, body: pageBody(options.view), scriptUri });
    this.panel.webview.onDidReceiveMessage((raw: unknown) => {
      const message = parseWebviewMessage(raw);
      if (message !== undefined) {
        observer?.({ view: options.view, direction: 'in', type: message.type });
        void Promise.resolve(options.onMessage(message)).catch((error: unknown) => {
          void vscode.window.showErrorMessage(`LP: ${error instanceof Error ? error.message : String(error)}`);
        });
      }
    });
    this.panel.onDidDispose(() => {
      this.disposed = true;
      options.onDispose();
    });
  }

  reveal(): void {
    this.panel.reveal(undefined, true);
  }

  get isDisposed(): boolean {
    return this.disposed;
  }

  setTitle(title: string): void {
    this.panel.title = title;
  }

  /** Sends parsed, sanitised data. `stale` shows the "document changed" banner. */
  data(result: unknown, extra: Record<string, unknown> = {}): void {
    observer?.({ view: this.view, direction: 'out', type: 'data', stale: extra.stale === true });
    void this.panel.webview.postMessage({ type: 'data', result, ...extra });
  }

  error(message: string): void {
    observer?.({ view: this.view, direction: 'out', type: 'error' });
    void this.panel.webview.postMessage({ type: 'error', message });
  }

  busy(on: boolean): void {
    void this.panel.webview.postMessage({ type: 'busy', on });
  }

  dispose(): void {
    if (!this.disposed) {
      this.panel.dispose();
    }
  }
}
