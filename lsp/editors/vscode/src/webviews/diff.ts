import { DiffChangeKind, DiffResult } from '../pure/protocol';
import { DiffRow, diffSummary, filterDiffRows, flattenDiff } from '../pure/diffView';
import { activatable, byId, clear, h, onHostMessage, post } from './common';

const RENDER_CAP = 2000;

const summary = byId<HTMLSpanElement>('summary');
const banner = byId<HTMLDivElement>('banner');
const body = byId<HTMLTableSectionElement>('rows');
const filterInput = byId<HTMLInputElement>('filter');
const checkboxes: Record<DiffChangeKind, HTMLInputElement> = {
  added: byId<HTMLInputElement>('show-added'),
  removed: byId<HTMLInputElement>('show-removed'),
  changed: byId<HTMLInputElement>('show-changed'),
};

let rows: DiffRow[] = [];

function render(): void {
  const changes = new Set<DiffChangeKind>((Object.keys(checkboxes) as DiffChangeKind[]).filter((key) => checkboxes[key].checked));
  const shown = filterDiffRows(rows, { text: filterInput.value, changes });
  clear(body);
  for (const row of shown.slice(0, RENDER_CAP)) {
    const symbol = row.change === 'added' ? '+' : row.change === 'removed' ? '−' : '~';
    const tr = h(
      'tr',
      {},
      h('td', { class: row.change, text: symbol, title: row.change }),
      h('td', { class: 'muted', text: row.section }),
      h('td', { class: 'name', text: row.name }),
      h('td', { class: 'mono', text: row.detail }),
    );
    if (row.navigable) {
      activatable(tr, `Go to ${row.name}`, () => post({ type: 'revealItem', group: row.group, index: row.index }));
    }
    body.append(tr);
  }
  if (shown.length > RENDER_CAP) {
    body.append(h('tr', {}, h('td', { class: 'muted', text: `Showing the first ${RENDER_CAP} of ${shown.length} rows. Narrow the filter.` })));
  }
}

for (const box of Object.values(checkboxes)) {
  box.addEventListener('change', render);
}
filterInput.addEventListener('input', render);
byId('refresh').addEventListener('click', () => post({ type: 'refresh' }));

onHostMessage({
  data: (message) => {
    const result = message.result as DiffResult;
    rows = flattenDiff(result);
    summary.textContent = `Against ${result.label.length > 0 ? result.label : 'the other model'}: ${diffSummary(result)}`;
    banner.hidden = message.stale !== true && !result.truncated;
    banner.textContent = message.stale === true ? 'The document changed since this was computed. Refresh to update.' : 'Some rows were dropped by the size limits.';
    render();
  },
  error: (text) => {
    rows = [];
    clear(body);
    banner.hidden = false;
    banner.textContent = text;
    summary.textContent = '';
  },
});
post({ type: 'ready' });
