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

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  extensionContext = context;
  outputChannel = vscode.window.createOutputChannel(CLIENT_NAME);
  lpFileWatcher = vscode.workspace.createFileSystemWatcher('**/*.lp');
  context.subscriptions.push(outputChannel, lpFileWatcher);

  context.subscriptions.push(
    vscode.commands.registerCommand('lp.restartServer', restartServer),
    vscode.commands.registerCommand('lp.showReferences', showReferences),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration('lp.server.path')) {
        void vscode.window
          .showInformationMessage('The LP server path changed. Restart the language server?', 'Restart')
          .then((choice) => (choice === 'Restart' ? restartServer() : undefined));
      }
    }),
  );

  client = createClient();
  await startClient(client);
}

export async function deactivate(): Promise<void> {
  if (client !== undefined) {
    await client.stop();
    client = undefined;
  }
}

type ServerSource = 'setting' | 'bundled' | 'PATH';

/** The server to run: the `lp.server.path` setting, then the binary bundled in the extension, then `PATH`. */
function serverPath(): { command: string; source: ServerSource } {
  const context = extensionContext;
  const configured = vscode.workspace.getConfiguration('lp').get<string>('server.path', '').trim();
  if (configured.length > 0) {
    return { command: configured, source: 'setting' };
  }
  const exe = process.platform === 'win32' ? 'lp-lsp.exe' : 'lp-lsp';
  const bundled = context.asAbsolutePath(path.join('server', exe));
  return fs.existsSync(bundled) ? { command: bundled, source: 'bundled' } : { command: 'lp-lsp', source: 'PATH' };
}

function createClient(): LanguageClient {
  const { command, source } = serverPath();
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

  return new LanguageClient(CLIENT_ID, CLIENT_NAME, serverOptions, clientOptions);
}

async function startClient(languageClient: LanguageClient): Promise<void> {
  try {
    await languageClient.start();
    const info = languageClient.initializeResult?.serverInfo;
    outputChannel?.appendLine(`Server started: ${info?.name ?? 'unknown'} ${info?.version ?? 'unknown version'}`);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    outputChannel?.appendLine(`Failed to start '${serverPath().command}': ${message}`);
    void vscode.window.showErrorMessage(`Could not start the LP language server '${serverPath().command}': ${message}. ${INSTALL_HINT}`);
  }
}

async function restartServer(): Promise<void> {
  if (client === undefined) {
    return;
  }
  const previous = client;
  // Recreate the client so a changed `lp.server.path` takes effect.
  client = undefined;
  try {
    await previous.dispose();
  } catch (error) {
    outputChannel?.appendLine(`Error while stopping the server: ${String(error)}`);
  }
  client = createClient();
  await startClient(client);
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
