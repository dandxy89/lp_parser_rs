import * as vscode from 'vscode';
import { LP_REQUESTS, LpCapabilities, supportsContextKey } from './pure/capabilities';

/** Publishes the advertised requests as context keys; `enablement` in package.json gates each command on them. */
export async function publishCapabilities(capabilities: LpCapabilities): Promise<void> {
  for (const request of LP_REQUESTS) {
    await vscode.commands.executeCommand('setContext', supportsContextKey(request), capabilities.has(request));
  }
}
