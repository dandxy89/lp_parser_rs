import * as fs from 'node:fs';
import * as path from 'node:path';
import * as vscode from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  Location as LspLocation,
  Position as LspPosition,
  ServerOptions,
  TransportKind,
} from 'vscode-languageclient/node';
import { publishCapabilities } from './commands';
import { LpRequest, parseLpCapabilities } from './pure/capabilities';
import { chooseServer, RESTRICTED_NO_SERVER, ServerChoice } from './pure/serverPath';
import { createModelClient } from './modelClient';
import { ModelViews, registerModelViews } from './modelViews';
import { LpStatusBar } from './statusBar';
import { setClientProvider } from './views/lpClient';
import { registerViews } from './views';
import { observePanels, PanelEvent } from './views/panel';

const CLIENT_ID = 'lp';
const CLIENT_NAME = 'LP Language Server';
const INSTALL_HINT = 'Install it with `cargo install --locked lp-lsp` or set `lp.server.path`.';

/** Server commands whose result is `{ markdown }`. */
const REPORT_COMMANDS = new Set(['lp.analyze', 'lp.showModelStats']);
/** Server commands that take the active document's URI as their first argument. */
const DOCUMENT_COMMANDS = new Set(['lp.analyze', 'lp.convertToMps', 'lp.showModelStats']);

let extensionContext: vscode.ExtensionContext;
let client: LanguageClient | undefined;
let outputChannel: vscode.OutputChannel | undefined;
let lpFileWatcher: vscode.FileSystemWatcher | undefined;
let statusBar: LpStatusBar | undefined;
let modelViews: ModelViews | undefined;
/** The restart in flight, so a trust grant and a manual restart do not start two servers. */
let restarting: Promise<void> | undefined;

/** What `activate` returns: read access for the extension test host. Not a supported public API. */
export interface LpExtensionApi {
  readonly explorerRoots: ModelViews['explorerRoots'];
  readonly analysisTree: ModelViews['analysisTree'];
  readonly observePanels: (observer: ((event: PanelEvent) => void) | undefined) => void;
}

export async function activate(context: vscode.ExtensionContext): Promise<LpExtensionApi> {
  extensionContext = context;
  outputChannel = vscode.window.createOutputChannel(CLIENT_NAME);
  lpFileWatcher = vscode.workspace.createFileSystemWatcher('**/*.lp');
  statusBar = new LpStatusBar();
  context.subscriptions.push(outputChannel, lpFileWatcher, statusBar);
  modelViews = registerModelViews(context, createModelClient(() => client), statusBar, (message) => outputChannel?.appendLine(message));
  setClientProvider(() => client);
  registerViews(context);

  context.subscriptions.push(
    vscode.commands.registerCommand('lp.restartServer', restartServer),
    vscode.commands.registerCommand('lp.showReferences', showReferences),
    vscode.workspace.onDidGrantWorkspaceTrust(() => void restartServer()),
    vscode.workspace.onDidChangeConfiguration((event) => {
      // In Restricted Mode the setting is ignored, so there is nothing to apply.
      if (event.affectsConfiguration('lp.server.path') && vscode.workspace.isTrusted) {
        void vscode.window
          .showInformationMessage('The LP server path changed. Restart the language server?', 'Restart')
          .then((choice) => (choice === 'Restart' ? restartServer() : undefined));
      }
    }),
  );

  await launchClient();
  return {
    explorerRoots: () => modelViews?.explorerRoots() ?? Promise.resolve([]),
    analysisTree: () => modelViews?.analysisTree() ?? [],
    observePanels,
  };
}

export async function deactivate(): Promise<void> {
  if (client !== undefined) {
    await client.stop();
    client = undefined;
  }
  statusBar?.setState('stopped');
}

/** The server to run (see `chooseServer`); `undefined` when Restricted Mode leaves none. */
function serverPath(): ServerChoice | undefined {
  const exe = process.platform === 'win32' ? 'lp-lsp.exe' : 'lp-lsp';
  const bundled = extensionContext.asAbsolutePath(path.join('server', exe));
  return chooseServer({
    configured: vscode.workspace.getConfiguration('lp').get<string>('server.path', ''),
    bundled,
    bundledExists: fs.existsSync(bundled),
    trusted: vscode.workspace.isTrusted,
  });
}

/** `undefined` (after telling the user) when Restricted Mode leaves no server that may run. */
function createClient(): { client: LanguageClient; command: string } | undefined {
  const choice = serverPath();
  if (choice === undefined) {
    outputChannel?.appendLine('Restricted Mode: no bundled server, nothing started');
    statusBar?.setState('failed');
    void publishCapabilities(new Set<LpRequest>());
    void vscode.window
      .showWarningMessage(RESTRICTED_NO_SERVER, 'Manage Workspace Trust')
      .then((picked) => (picked === undefined ? undefined : vscode.commands.executeCommand('workbench.trust.manage')));
    return undefined;
  }
  const { command, source } = choice;
  outputChannel?.appendLine(`${process.platform}-${process.arch}; server: ${command} (${source})`);
  const serverOptions: ServerOptions = {
    run: { command, transport: TransportKind.stdio },
    debug: { command, transport: TransportKind.stdio },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: 'file', language: 'lp' },
      { scheme: 'untitled', language: 'lp' },
    ],
    synchronize: {
      configurationSection: 'lp',
      fileEvents: lpFileWatcher,
    },
    outputChannel,
    initializationOptions: { workspaceTrusted: vscode.workspace.isTrusted },
    middleware: {
      // The client registers every command the server advertises; this hook
      // supplies the active document when a command is run from the palette
      // and presents markdown reports.
      executeCommand: async (command, args, next) => {
        const fullArgs = DOCUMENT_COMMANDS.has(command) && args.length === 0 ? activeDocumentArgs() : args;
        if (fullArgs === undefined) {
          return undefined;
        }
        const result: unknown = await next(command, fullArgs);
        if (REPORT_COMMANDS.has(command)) {
          await showMarkdownReport(result);
        }
        return result;
      },
    },
  };

  return { client: new LanguageClient(CLIENT_ID, CLIENT_NAME, serverOptions, clientOptions), command };
}

async function startClient(languageClient: LanguageClient, command: string): Promise<void> {
  statusBar?.setState('starting');
  try {
    await languageClient.start();
    const info = languageClient.initializeResult?.serverInfo;
    outputChannel?.appendLine(`Server started: ${info?.name ?? 'unknown'} ${info?.version ?? 'unknown version'}`);
    statusBar?.setState('running');
    await publishCapabilities(parseLpCapabilities(languageClient.initializeResult?.capabilities.experimental));
    modelViews?.refresh();
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    outputChannel?.appendLine(`Failed to start '${command}': ${message}`);
    statusBar?.setState('failed');
    await publishCapabilities(new Set<LpRequest>());
    void vscode.window.showErrorMessage(`Could not start the LP language server '${command}': ${message}. ${INSTALL_HINT}`);
  }
}

/** Create and start the client; leaves `client` unset when Restricted Mode allows no server. */
async function launchClient(): Promise<void> {
  const created = createClient();
  if (created === undefined) {
    return;
  }
  client = created.client;
  await startClient(created.client, created.command);
}

async function restartServer(): Promise<void> {
  if (restarting !== undefined) {
    return restarting;
  }
  restarting = (async () => {
    const previous = client;
    // Recreate the client so a changed `lp.server.path` or a granted trust takes effect.
    client = undefined;
    try {
      await previous?.dispose();
    } catch (error) {
      outputChannel?.appendLine(`Error while stopping the server: ${String(error)}`);
    }
    await launchClient();
  })();
  try {
    await restarting;
  } finally {
    restarting = undefined;
  }
}

/** Arguments for a document command: the active LP document's URI. */
function activeDocumentArgs(): unknown[] | undefined {
  const editor = vscode.window.activeTextEditor;
  if (editor === undefined || editor.document.languageId !== 'lp') {
    void vscode.window.showWarningMessage('Open an LP file to run this command.');
    return undefined;
  }
  return [editor.document.uri.toString()];
}

async function showMarkdownReport(result: unknown): Promise<void> {
  const markdown = (result as { markdown?: unknown } | null | undefined)?.markdown;
  if (typeof markdown !== 'string') {
    return;
  }
  const document = await vscode.workspace.openTextDocument({ language: 'markdown', content: markdown });
  try {
    await vscode.commands.executeCommand('markdown.showPreview', document.uri);
  } catch {
    // Markdown preview unavailable (extension disabled): show the source instead.
    await vscode.window.showTextDocument(document, { preview: true });
  }
}

/** Client-side `lp.showReferences`: arguments are `[uri, position, locations]` in LSP JSON form. */
async function showReferences(uri: unknown, position: unknown, locations: unknown): Promise<void> {
  if (client === undefined || typeof uri !== 'string' || !Array.isArray(locations)) {
    outputChannel?.appendLine('lp.showReferences: invalid arguments');
    return;
  }
  const converter = client.protocol2CodeConverter;
  await vscode.commands.executeCommand(
    'editor.action.showReferences',
    converter.asUri(uri),
    converter.asPosition(position as LspPosition),
    (locations as LspLocation[]).map((location) => converter.asLocation(location)),
  );
}
