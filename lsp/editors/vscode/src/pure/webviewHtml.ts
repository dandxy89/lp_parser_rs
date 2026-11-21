import { escapeHtml } from './sanitise';

/** A 128-bit nonce, base64url, from caller-supplied random bytes (so tests are deterministic). */
export function makeNonce(randomBytes: (n: number) => Uint8Array): string {
  const bytes = randomBytes(16);
  if (bytes.length !== 16) {
    throw new Error('makeNonce: expected 16 random bytes');
  }
  return Buffer.from(bytes).toString('base64url');
}

export function isValidNonce(nonce: string): boolean {
  return /^[A-Za-z0-9_-]{16,}$/.test(nonce);
}

/**
 * The strict policy every LP webview runs under: nothing loads unless it carries this page's nonce, no
 * network, no frames, no forms, no `eval`. Images are limited to `data:` URIs (canvas export).
 */
export function contentSecurityPolicy(nonce: string): string {
  if (!isValidNonce(nonce)) {
    throw new Error('contentSecurityPolicy: invalid nonce');
  }
  return [
    "default-src 'none'",
    `style-src 'nonce-${nonce}'`,
    `script-src 'nonce-${nonce}'`,
    'img-src data:',
    "base-uri 'none'",
    "form-action 'none'",
    "frame-ancestors 'none'",
  ].join('; ');
}

export interface PageOptions {
  /** Plain text; escaped here. */
  readonly title: string;
  readonly nonce: string;
  /** CSS text for the inline `<style>`. Trusted, static. */
  readonly css: string;
  /** Static body markup. Trusted: never put model names in it (send them with `postMessage`). */
  readonly body: string;
  /** URI of the bundled script, already converted with `webview.asWebviewUri`. */
  readonly scriptUri: string;
}

/** Characters that could end an attribute or tag if they appeared in a URI. */
const UNSAFE_URI = /["'<>`\s]/;

export function buildPage(options: PageOptions): string {
  if (UNSAFE_URI.test(options.scriptUri)) {
    throw new Error('buildPage: script URI contains unsafe characters');
  }
  if (/<\/style/i.test(options.css)) {
    throw new Error('buildPage: css must not close the style element');
  }
  const csp = contentSecurityPolicy(options.nonce);
  return `<!DOCTYPE html>
<html lang="en-GB">
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="${escapeHtml(csp)}">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escapeHtml(options.title)}</title>
<style nonce="${options.nonce}">${options.css}</style>
</head>
<body>
${options.body}
<script nonce="${options.nonce}" src="${options.scriptUri}"></script>
</body>
</html>
`;
}
