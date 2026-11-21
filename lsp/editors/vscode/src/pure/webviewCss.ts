/** Shared stylesheet for every LP webview. Static and trusted; uses the editor theme's variables only. */
export const WEBVIEW_CSS = `
:root { color-scheme: light dark; }
html, body { height: 100%; }
body { margin: 0; padding: 0; font-family: var(--vscode-font-family); font-size: var(--vscode-font-size); color: var(--vscode-foreground); background: var(--vscode-editor-background); display: flex; flex-direction: column; }
header { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; padding: 8px 12px; border-bottom: 1px solid var(--vscode-panel-border); }
header h1 { font-size: 1.1em; margin: 0 8px 0 0; font-weight: 600; }
main { flex: 1; min-height: 0; overflow: auto; padding: 8px 12px; }
input[type=search], input[type=text], select { background: var(--vscode-input-background); color: var(--vscode-input-foreground); border: 1px solid var(--vscode-input-border, transparent); padding: 3px 6px; border-radius: 2px; }
button { background: var(--vscode-button-secondaryBackground); color: var(--vscode-button-secondaryForeground); border: 0; padding: 3px 10px; border-radius: 2px; cursor: pointer; }
button:hover:not(:disabled) { background: var(--vscode-button-secondaryHoverBackground); }
button:disabled { opacity: 0.5; cursor: default; }
button:focus-visible, input:focus-visible, select:focus-visible, tr:focus-visible, summary:focus-visible { outline: 1px solid var(--vscode-focusBorder); }
.muted { color: var(--vscode-descriptionForeground); }
.banner { padding: 4px 12px; background: var(--vscode-inputValidation-warningBackground); border-bottom: 1px solid var(--vscode-inputValidation-warningBorder); }
.error { color: var(--vscode-errorForeground); }
.ok { color: var(--vscode-testing-iconPassed, #2e9d56); }
.warn { color: var(--vscode-editorWarning-foreground); }
.bad { color: var(--vscode-errorForeground); }
canvas { display: block; width: 100%; height: 100%; }
.stage { position: relative; flex: 1; min-height: 0; }
.tip { position: absolute; pointer-events: none; padding: 2px 6px; background: var(--vscode-editorHoverWidget-background); border: 1px solid var(--vscode-editorHoverWidget-border); border-radius: 2px; white-space: pre; max-width: 60ch; overflow: hidden; text-overflow: ellipsis; }
table { border-collapse: collapse; width: 100%; }
th, td { text-align: left; padding: 2px 8px; border-bottom: 1px solid var(--vscode-panel-border); vertical-align: top; }
th { position: sticky; top: 0; background: var(--vscode-editor-background); cursor: default; }
tr.link { cursor: pointer; }
tr.link:hover { background: var(--vscode-list-hoverBackground); }
td.num { text-align: right; font-family: var(--vscode-editor-font-family); }
td.name, .mono { font-family: var(--vscode-editor-font-family); overflow-wrap: anywhere; }
.added { color: var(--vscode-gitDecoration-addedResourceForeground, #2e9d56); }
.removed { color: var(--vscode-gitDecoration-deletedResourceForeground, #c74e39); }
.changed { color: var(--vscode-gitDecoration-modifiedResourceForeground, #4e94ce); }
details { margin-left: 12px; }
details > summary { cursor: pointer; margin-left: -12px; }
.pre { white-space: pre-wrap; overflow-wrap: anywhere; }
.legend { display: flex; gap: 12px; align-items: center; }
.swatch { display: inline-block; width: 10px; height: 10px; border-radius: 2px; margin-right: 4px; }
`;
