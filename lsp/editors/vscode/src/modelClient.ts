import type { LanguageClient } from 'vscode-languageclient/node';
import type { EntityKind, ExplainResult, ModelClient, ModelResult } from './pure/modelTypes';

/** Adapts the language client to the `ModelClient` the pure explorer logic uses. `getClient` is read on every call so restarts are followed. */
export function createModelClient(getClient: () => LanguageClient | undefined): ModelClient {
  const running = (): LanguageClient => {
    const client = getClient();
    if (client === undefined || !client.isRunning()) {
      throw new Error('the language server is not running');
    }
    return client;
  };
  return {
    model: (uri, include): Promise<ModelResult> =>
      running().sendRequest<ModelResult>('lp/model', { textDocument: { uri }, ...(include === undefined ? {} : { include }) }),
    explain: (uri, target: { kind: EntityKind; name: string }): Promise<ExplainResult> =>
      running().sendRequest<ExplainResult>('lp/explain', { textDocument: { uri }, target }),
  };
}
