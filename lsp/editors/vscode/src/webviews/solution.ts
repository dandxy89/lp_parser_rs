import { SolveResult } from '../pure/protocol';
import {
  SortKey,
  TreeNode,
  constraintRows,
  formatNumber,
  hasDuals,
  infeasibilityTree,
  statusTone,
  summaryLines,
  variableRows,
} from '../pure/solutionView';
import { activatable, byId, clear, h, onHostMessage, post } from './common';

const RENDER_CAP = 2000;

const banner = byId<HTMLDivElement>('banner');
const summary = byId<HTMLDivElement>('summary');
const tree = byId<HTMLDivElement>('tree');
const head = byId<HTMLTableSectionElement>('head');
const body = byId<HTMLTableSectionElement>('rows');
const tab = byId<HTMLSelectElement>('tab');
const filterInput = byId<HTMLInputElement>('filter');
const narrow = byId<HTMLInputElement>('narrow');
const narrowLabel = byId<HTMLSpanElement>('narrow-label');
const sortSelect = byId<HTMLSelectElement>('sort');

let result: SolveResult | undefined;

function options(): { filter: string; sort: SortKey; narrow: boolean } {
  return { filter: filterInput.value, sort: sortSelect.value as SortKey, narrow: narrow.checked };
}

function header(...labels: string[]): void {
  clear(head);
  head.append(h('tr', {}, ...labels.map((label) => h('th', { text: label }))));
}

function renderTable(): void {
  clear(body);
  if (result === undefined) {
    return;
  }
  const showDuals = hasDuals(result);
  if (tab.value === 'variables') {
    narrowLabel.textContent = 'Non-zero only';
    header('Variable', 'Value', ...(showDuals ? ['Reduced cost'] : []));
    const rows = variableRows(result, options());
    for (const { source, variable } of rows.slice(0, RENDER_CAP)) {
      const tr = h(
        'tr',
        {},
        h('td', { class: 'name', text: variable.name }),
        h('td', { class: 'num', text: formatNumber(variable.value) }),
        showDuals ? h('td', { class: 'num', text: variable.reducedCost === undefined ? '' : formatNumber(variable.reducedCost) }) : null,
      );
      if (variable.range !== undefined) {
        activatable(tr, `Go to ${variable.name}`, () => post({ type: 'revealItem', group: 'variables', index: source }));
      }
      body.append(tr);
    }
    capNote(rows.length);
  } else {
    narrowLabel.textContent = 'Binding only';
    header('Constraint', 'Activity', 'Slack', ...(showDuals ? ['Dual'] : []), 'Binding');
    const rows = constraintRows(result, options());
    for (const { source, constraint } of rows.slice(0, RENDER_CAP)) {
      const tr = h(
        'tr',
        {},
        h('td', { class: 'name', text: constraint.name }),
        h('td', { class: 'num', text: formatNumber(constraint.activity) }),
        h('td', { class: 'num', text: formatNumber(constraint.slack) }),
        showDuals ? h('td', { class: 'num', text: constraint.dual === undefined ? '' : formatNumber(constraint.dual) }) : null,
        h('td', { text: constraint.binding ? 'yes' : '' }),
      );
      if (constraint.range !== undefined) {
        activatable(tr, `Go to ${constraint.name}`, () => post({ type: 'revealItem', group: 'constraints', index: source }));
      }
      body.append(tr);
    }
    capNote(rows.length);
  }
}

function capNote(total: number): void {
  if (total > RENDER_CAP) {
    body.append(h('tr', {}, h('td', { class: 'muted', text: `Showing the first ${RENDER_CAP} of ${total} rows. Use the filter.` })));
  }
}

function renderTree(node: TreeNode, root = true): HTMLElement {
  const label = h('span', {}, h('span', { class: 'mono', text: node.label }), node.detail ? h('span', { class: 'muted', text: `  ${node.detail}` }) : null);
  const clickable = node.group !== undefined && node.index !== undefined;
  if (node.children.length === 0) {
    const leaf = h('div', {}, label);
    if (clickable) {
      activatable(leaf, `Go to ${node.label}`, () => post({ type: 'revealItem', group: node.group, index: node.index }));
    }
    return leaf;
  }
  const details = h('details', {}, h('summary', {}, label));
  details.open = root || node.children.length <= 20;
  for (const child of node.children) {
    details.append(renderTree(child, false));
  }
  return details;
}

function render(): void {
  if (result === undefined) {
    return;
  }
  clear(summary);
  const lines = summaryLines(result);
  lines.forEach((line, index) => summary.append(h('div', { class: index === 0 ? statusTone(result!.status) : '', text: line })));
  clear(tree);
  if (result.infeasibility !== undefined) {
    tree.append(h('h2', { text: 'Why is it infeasible?' }), renderTree(infeasibilityTree(result.infeasibility)));
  }
  renderTable();
}

for (const control of [tab, narrow, sortSelect]) {
  control.addEventListener('change', renderTable);
}
filterInput.addEventListener('input', renderTable);
byId('refresh').addEventListener('click', () => post({ type: 'refresh' }));

onHostMessage({
  data: (message) => {
    result = message.result as SolveResult;
    banner.hidden = message.stale !== true && !result.truncated;
    banner.textContent = message.stale === true ? 'The document changed since this solve. Solve again to update.' : 'Some rows were dropped by the size limits.';
    render();
  },
  error: (text) => {
    result = undefined;
    banner.hidden = false;
    banner.textContent = text;
    clear(summary);
    clear(tree);
    clear(body);
  },
  busy: (on) => {
    byId<HTMLButtonElement>('refresh').disabled = on;
  },
});
post({ type: 'ready' });
