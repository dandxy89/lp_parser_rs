import { ExplainResult } from '../pure/protocol';
import { explainFacts } from '../pure/explainView';
import { formatNumber } from '../pure/solutionView';
import { activatable, byId, clear, h, onHostMessage, post } from './common';

const content = byId<HTMLDivElement>('content');
const banner = byId<HTMLDivElement>('banner');

function section(title: string, ...children: (Node | null)[]): HTMLElement {
  return h('section', {}, h('h2', { text: title }), ...children);
}

function render(result: ExplainResult): void {
  clear(content);
  content.append(h('h1', { class: 'mono', text: `${result.kind} ${result.name}` }));
  if (result.range !== undefined) {
    content.append(h('button', { text: 'Go to source', onclick: () => post({ type: 'revealItem', group: 'explain', index: 0 }) }));
  }
  // Markdown is shown as plain text: a webview that renders model-derived markdown would be an injection surface.
  if (result.markdown.length > 0) {
    content.append(h('p', { class: 'pre', text: result.markdown }));
  }
  const facts = explainFacts(result);
  if (facts.length > 0) {
    content.append(h('ul', {}, ...facts.map((fact) => h('li', { text: fact }))));
  }
  if (result.terms.length > 0) {
    const rows = result.terms.map((term, index) => {
      const tr = h('tr', {}, h('td', { class: 'name', text: term.variable }), h('td', { class: 'num', text: formatNumber(term.coefficient) }));
      activatable(tr, `Explain ${term.variable}`, () => post({ type: 'explain', kind: 'variable', name: term.variable }));
      return index < 2000 ? tr : null;
    });
    content.append(
      section(
        `Terms (${result.terms.length}${result.termsTruncated ? '+' : ''})`,
        h('table', {}, h('thead', {}, h('tr', {}, h('th', { text: 'Variable' }), h('th', { text: 'Coefficient' }))), h('tbody', {}, ...rows)),
      ),
    );
  }
  if (result.usage.length > 0) {
    const rows = result.usage.slice(0, 2000).map((use) => {
      const tr = h(
        'tr',
        {},
        h('td', { class: 'name', text: use.entity }),
        h('td', { class: 'muted', text: use.role }),
        h('td', { class: 'num', text: use.coefficient === undefined ? '' : formatNumber(use.coefficient) }),
      );
      if (use.kind === 'constraint' || use.kind === 'objective') {
        activatable(tr, `Explain ${use.entity}`, () => post({ type: 'explain', kind: use.kind as 'constraint' | 'objective', name: use.entity }));
      }
      return tr;
    });
    content.append(
      section(
        `Used in (${result.usage.length})`,
        h('table', {}, h('thead', {}, h('tr', {}, h('th', { text: 'Entity' }), h('th', { text: 'Role' }), h('th', { text: 'Coefficient' }))), h('tbody', {}, ...rows)),
      ),
    );
  }
  if (result.related.length > 0) {
    const rows = result.related.map((rel) => {
      const tr = h('tr', {}, h('td', { class: 'name', text: rel.name }), h('td', { class: 'muted', text: `shares ${rel.shared} variables` }));
      if (rel.kind === 'constraint' || rel.kind === 'objective') {
        activatable(tr, `Explain ${rel.name}`, () => post({ type: 'explain', kind: rel.kind as 'constraint' | 'objective', name: rel.name }));
      }
      return tr;
    });
    content.append(section('Related constraints', h('table', {}, h('tbody', {}, ...rows))));
  }
}

onHostMessage({
  data: (message) => {
    banner.hidden = true;
    render(message.result as ExplainResult);
  },
  error: (text) => {
    clear(content);
    banner.hidden = false;
    banner.textContent = text;
  },
});
post({ type: 'ready' });
