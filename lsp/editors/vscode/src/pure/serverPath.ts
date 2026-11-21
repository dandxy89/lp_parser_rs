export type ServerSource = 'setting' | 'bundled' | 'PATH';

export interface ServerChoice {
  readonly command: string;
  readonly source: ServerSource;
}

export interface ServerInputs {
  /** `lp.server.path`, untrimmed. */
  readonly configured: string;
  /** Absolute path of `<extension>/server/lp-lsp[.exe]`. */
  readonly bundled: string;
  readonly bundledExists: boolean;
  /** `vscode.workspace.isTrusted`. */
  readonly trusted: boolean;
}

/** Shown when Restricted Mode leaves nothing that may run. */
export const RESTRICTED_NO_SERVER =
  'LP: Restricted Mode runs only the language server bundled with the extension, and this build has none. Trust the workspace to use lp.server.path or PATH.';

/**
 * The server to run: the `lp.server.path` setting, then the bundled binary, then `PATH`.
 * In an untrusted workspace only the bundled binary may run; `undefined` means nothing may.
 */
export function chooseServer(inputs: ServerInputs): ServerChoice | undefined {
  if (!inputs.trusted) {
    return inputs.bundledExists ? { command: inputs.bundled, source: 'bundled' } : undefined;
  }
  const configured = inputs.configured.trim();
  if (configured.length > 0) {
    return { command: configured, source: 'setting' };
  }
  return inputs.bundledExists ? { command: inputs.bundled, source: 'bundled' } : { command: 'lp-lsp', source: 'PATH' };
}
