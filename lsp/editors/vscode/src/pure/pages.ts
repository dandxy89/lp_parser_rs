/** Static markup for each webview body. No model data ever goes here; it arrives with `postMessage`. */

export type ViewName = 'matrix' | 'graph' | 'diff' | 'solution' | 'explain';

const matrix = `
<header>
  <h1>Coefficient Matrix</h1>
  <button id="overview" title="Density overview of the whole matrix">Overview</button>
  <button id="detail" title="Individual coefficients">Detail</button>
  <button id="up" title="Previous rows" aria-label="Previous rows">▲</button>
  <button id="down" title="Next rows" aria-label="Next rows">▼</button>
  <button id="left" title="Previous columns" aria-label="Previous columns">◀</button>
  <button id="right" title="Next columns" aria-label="Next columns">▶</button>
  <button id="refresh">Refresh</button>
  <span id="info" class="muted" role="status"></span>
</header>
<div id="banner" class="banner" role="status" hidden></div>
<div class="stage" id="stage"><canvas id="canvas" role="img" aria-label="Coefficient matrix"></canvas><div id="tip" class="tip" hidden></div></div>`;

const graph = `
<header>
  <h1>Dependency Graph</h1>
  <input id="search" type="search" placeholder="Search names" aria-label="Search names">
  <select id="component" aria-label="Component"><option value="">All components</option></select>
  <button id="reveal" title="Open the selected node in the editor">Go to source</button>
  <label>Depth <select id="depth" aria-label="Focus depth"><option>1</option><option>2</option><option>3</option></select></label>
  <button id="focus" title="Show only the neighbourhood of the selected node">Focus</button>
  <button id="clearFocus">Show all</button>
  <button id="refresh">Refresh</button>
  <span id="info" class="muted" role="status"></span>
</header>
<div id="banner" class="banner" role="status" hidden></div>
<div class="stage" id="stage"><canvas id="canvas" role="img" aria-label="Bipartite graph of constraints and variables. Constraints on the left, variables on the right."></canvas></div>`;

const diff = `
<header>
  <h1>Model Diff</h1>
  <span id="summary" role="status"></span>
  <input id="filter" type="search" placeholder="Filter" aria-label="Filter">
  <label><input type="checkbox" id="show-added" checked> Added</label>
  <label><input type="checkbox" id="show-removed" checked> Removed</label>
  <label><input type="checkbox" id="show-changed" checked> Changed</label>
  <button id="refresh">Refresh</button>
</header>
<div id="banner" class="banner" role="status" hidden></div>
<main><table><thead><tr><th></th><th>Kind</th><th>Name</th><th>Change</th></tr></thead><tbody id="rows"></tbody></table></main>`;

const solution = `
<header>
  <h1>Solution</h1>
  <select id="tab" aria-label="Table"><option value="variables">Variables</option><option value="constraints">Constraints</option></select>
  <input id="filter" type="search" placeholder="Filter" aria-label="Filter">
  <label><input type="checkbox" id="narrow"> <span id="narrow-label">Non-zero only</span></label>
  <select id="sort" aria-label="Sort">
    <option value="model">Model order</option><option value="name">Name</option><option value="value">Value</option>
    <option value="slack">Slack</option><option value="dual">Dual / reduced cost</option>
  </select>
  <button id="refresh">Solve again</button>
</header>
<div id="banner" class="banner" role="status" hidden></div>
<main>
  <div id="summary" role="status"></div>
  <div id="tree"></div>
  <table><thead id="head"></thead><tbody id="rows"></tbody></table>
</main>`;

const explain = `
<div id="banner" class="banner error" role="alert" hidden></div>
<main id="content"></main>`;

const BODIES: Readonly<Record<ViewName, string>> = { matrix, graph, diff, solution, explain };

export function pageBody(view: ViewName): string {
  return BODIES[view];
}

export const VIEW_NAMES: readonly ViewName[] = ['matrix', 'graph', 'diff', 'solution', 'explain'];
