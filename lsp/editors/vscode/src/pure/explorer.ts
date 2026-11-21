import { LIMITS, cleanText } from './sanitise';
import type { ConstraintItem, EntityKind, LspRange, ModelClient, ModelResult, VariableItem } from './modelTypes';

/** Icon names are codicon ids, mapped to `ThemeIcon` by the view. */
export type NodeKind = 'section' | 'group' | 'objective' | 'constraint' | 'variable' | 'special' | 'term' | 'usage' | 'notice';

export interface ExplorerNode {
  /** Stable across refreshes so VS Code keeps expansion state. */
  readonly id: string;
  readonly kind: NodeKind;
  readonly label: string;
  readonly description?: string;
  readonly tooltip?: string;
  readonly icon: string;
  readonly expandable: boolean;
  /** Source range to reveal when the node is clicked. */
  readonly range?: LspRange;
  /** The entity whose children are fetched with `lp/explain`. */
  readonly entity?: { readonly kind: EntityKind; readonly name: string };
  /** Private payload for children: the group members. */
  readonly members?: readonly ModelMember[];
}

export type ModelMember = ConstraintItem | VariableItem | { readonly name: string; readonly range: LspRange | null; readonly terms: number; readonly description?: string };

const VARIABLE_KIND_ORDER = ['continuous', 'integer', 'general', 'binary', 'semiContinuous', 'semiInteger', 'sos'];

const VARIABLE_KIND_LABEL: Readonly<Record<string, string>> = {
  continuous: 'Continuous',
  integer: 'Integer',
  general: 'General',
  binary: 'Binary',
  semiContinuous: 'Semi-continuous',
  semiInteger: 'Semi-integer',
  sos: 'SOS members',
};

/** A constraint is "special" when it is not a plain linear row, or when it is a lazy or user cut. */
export function isSpecial(constraint: ConstraintItem): boolean {
  return constraint.kind !== 'linear' || constraint.class !== 'normal';
}

/** Formats a coefficient next to its variable: `2 x`, `-x`, `1.5 y`. `null` means the server sent a non-finite value. */
export function formatCoefficient(coefficient: number | null, variable: string): string {
  if (coefficient === null) {
    return `? ${variable}`;
  }
  if (coefficient === 1) {
    return variable;
  }
  if (coefficient === -1) {
    return `-${variable}`;
  }
  return `${coefficient} ${variable}`;
}

function formatBound(value: number | null, infinity: string): string {
  return value === null ? infinity : String(value);
}

function matches(name: string, filter: string): boolean {
  return filter.length === 0 || name.toLowerCase().includes(filter);
}

function count(total: number, shown: number, filtering: boolean): string {
  return filtering ? `${shown}/${total}` : String(total);
}

function notice(id: string, label: string, icon = 'info'): ExplorerNode {
  return { id, kind: 'notice', label, icon, expandable: false };
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Model Explorer logic with no dependency on the `vscode` module.
 * Roots come from one `lp/model` call per refresh; children of a variable or constraint come from `lp/explain`.
 */
export class ModelExplorer {
  private uri: string | undefined;
  private filter = '';
  private model: ModelResult | undefined;
  private loadError: string | undefined;
  /** Incremented on every refresh and document switch, so a slow reply for an old document is dropped. */
  private generation = 0;

  constructor(private readonly client: ModelClient) {}

  get documentUri(): string | undefined {
    return this.uri;
  }

  get currentModel(): ModelResult | undefined {
    return this.model;
  }

  /** Switches to another document (or none) and drops the loaded model. */
  setDocument(uri: string | undefined): void {
    if (uri !== this.uri) {
      this.uri = uri;
      this.invalidate();
    }
  }

  /** Marks the loaded model stale: the next `roots()` reloads it. */
  invalidate(): void {
    this.generation += 1;
    this.model = undefined;
    this.loadError = undefined;
  }

  /** Case-insensitive substring filter on entity names; empty clears it. */
  setFilter(text: string): void {
    this.filter = text.trim().toLowerCase();
  }

  get filterText(): string {
    return this.filter;
  }

  async roots(): Promise<ExplorerNode[]> {
    const uri = this.uri;
    if (uri === undefined) {
      return [notice('notice:no-document', 'Open an LP file to see its model.')];
    }
    if (this.model === undefined && this.loadError === undefined) {
      const generation = this.generation;
      try {
        const result = await this.client.model(uri);
        if (generation !== this.generation) {
          return this.roots();
        }
        this.model = result;
      } catch (error) {
        if (generation !== this.generation) {
          return this.roots();
        }
        this.loadError = errorMessage(error);
      }
    }
    if (this.loadError !== undefined) {
      return [notice('notice:error', `Could not load the model: ${this.loadError}`, 'error')];
    }
    return this.model === undefined ? [] : this.buildRoots(this.model);
  }

  async children(node: ExplorerNode): Promise<ExplorerNode[]> {
    const uri = this.uri;
    if (uri === undefined) {
      return [];
    }
    if (node.kind === 'section' || node.kind === 'group') {
      return this.memberNodes(node);
    }
    if (node.entity === undefined) {
      return [];
    }
    const generation = this.generation;
    try {
      const explained = await this.client.explain(uri, node.entity);
      if (generation !== this.generation) {
        return [];
      }
      const out: ExplorerNode[] = [];
      for (const [index, term] of (explained.terms ?? []).entries()) {
        out.push({
          id: `${node.id}/term:${index}`,
          kind: 'term',
          label: formatCoefficient(term.coefficient, term.variable),
          icon: 'symbol-variable',
          expandable: false,
          range: term.range ?? undefined,
        });
      }
      if (explained.termsTruncated === true) {
        out.push(notice(`${node.id}/truncated`, 'More terms not shown.'));
      }
      for (const [index, usage] of (explained.usage ?? []).entries()) {
        const coefficient = usage.coefficient === null ? '' : ` (${usage.coefficient})`;
        out.push({
          id: `${node.id}/usage:${index}`,
          kind: 'usage',
          label: usage.entity ?? '(unnamed)',
          description: `${usage.role}${coefficient}`,
          icon: usage.kind === 'objective' ? 'target' : 'symbol-operator',
          expandable: false,
          range: usage.range,
        });
      }
      if (explained.usageTruncated === true) {
        out.push(notice(`${node.id}/truncated`, 'More usages not shown.'));
      }
      return out.length === 0 ? [notice(`${node.id}/empty`, 'No terms.')] : out;
    } catch (error) {
      return [notice(`${node.id}/error`, `Could not explain ${node.entity.name}: ${errorMessage(error)}`, 'error')];
    }
  }

  private buildRoots(model: ModelResult): ExplorerNode[] {
    const filtering = this.filter.length > 0;
    const objectives = (model.objectives ?? []).filter((o) => matches(o.name, this.filter));
    const constraints = (model.constraints ?? []).filter((c) => !isSpecial(c) && matches(c.name, this.filter));
    const special = (model.constraints ?? []).filter((c) => isSpecial(c) && matches(c.name, this.filter));
    const variables = (model.variables ?? []).filter((v) => matches(v.name, this.filter));
    const totalConstraints = (model.constraints ?? []).filter((c) => !isSpecial(c)).length;
    const totalSpecial = (model.constraints ?? []).length - totalConstraints;

    const sections: ExplorerNode[] = [];
    const add = (id: string, label: string, icon: string, total: number, members: readonly ModelMember[], truncated: boolean): void => {
      if (members.length === 0 && (filtering || total === 0)) {
        return;
      }
      sections.push({
        id: `section:${id}`,
        kind: 'section',
        label,
        description: count(total, members.length, filtering) + (truncated ? '+' : ''),
        tooltip: truncated ? 'The server truncated this list.' : undefined,
        icon,
        expandable: members.length > 0,
        members,
      });
    };
    add('objectives', 'Objectives', 'target', model.objectives?.length ?? 0, objectives, model.truncated.objectives);
    add('constraints', 'Constraints', 'symbol-operator', totalConstraints, constraints, model.truncated.constraints);
    add('variables', 'Variables', 'symbol-variable', model.variables?.length ?? 0, variables, model.truncated.variables);
    add('special', 'Special', 'star', totalSpecial, special, false);
    if (sections.length === 0) {
      return [notice('notice:no-match', filtering ? `No entity matches "${this.filter}".` : 'The model is empty.')];
    }
    return sections;
  }

  private memberNodes(node: ExplorerNode): ExplorerNode[] {
    const members = node.members ?? [];
    if (node.id === 'section:variables') {
      return this.variableGroups(members as readonly VariableItem[]);
    }
    if (node.kind === 'group') {
      return (members as readonly VariableItem[]).map(variableNode);
    }
    if (node.id === 'section:objectives') {
      return members.map((m) => ({
        id: `objective:${m.name}`,
        kind: 'objective' as const,
        label: m.name,
        description: `${(m as { terms: number }).terms} terms`,
        ...withDescription(undefined, m.description),
        icon: 'target',
        expandable: true,
        range: m.range ?? undefined,
        entity: { kind: 'objective' as const, name: m.name },
      }));
    }
    return (members as readonly ConstraintItem[]).map(constraintNode);
  }

  private variableGroups(variables: readonly VariableItem[]): ExplorerNode[] {
    const byKind = new Map<string, VariableItem[]>();
    for (const variable of variables) {
      const list = byKind.get(variable.kind);
      if (list === undefined) {
        byKind.set(variable.kind, [variable]);
      } else {
        list.push(variable);
      }
    }
    const kinds = [...byKind.keys()].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
    return kinds.map((kind) => {
      const members = byKind.get(kind) ?? [];
      const total = this.model?.summary?.variablesByKind[kind];
      return {
        id: `group:variables:${kind}`,
        kind: 'group' as const,
        label: VARIABLE_KIND_LABEL[kind] ?? kind,
        description: count(total ?? members.length, members.length, this.filter.length > 0),
        icon: 'symbol-field',
        expandable: true,
        members,
      };
    });
  }
}

function rank(kind: string): number {
  const index = VARIABLE_KIND_ORDER.indexOf(kind);
  return index === -1 ? VARIABLE_KIND_ORDER.length : index;
}

/** Tooltip = `base` then the entry's description (plain text, re-cleaned here), or nothing when both are absent. */
function withDescription(base: string | undefined, description: string | undefined): { tooltip?: string } {
  const text = typeof description === 'string' && description.length > 0 ? cleanText(description, LIMITS.description) : undefined;
  const tooltip = base !== undefined && text !== undefined ? `${base}\n\n${text}` : (base ?? text);
  return tooltip === undefined ? {} : { tooltip };
}

function constraintNode(c: ConstraintItem): ExplorerNode {
  const relation = c.operator === undefined ? c.kind : `${c.operator} ${c.rhs === null || c.rhs === undefined ? '' : c.rhs}`.trim();
  const flags = [c.class !== 'normal' ? c.class : '', c.rangePartner !== null ? `range with ${c.rangePartner}` : ''].filter((f) => f.length > 0);
  return {
    id: `constraint:${c.name}`,
    kind: 'constraint',
    label: c.name,
    description: [relation, ...flags].join(' · '),
    ...withDescription(`${c.kind} constraint, ${c.terms} terms${c.generated ? ' (generated name)' : ''}`, c.description),
    icon: c.kind === 'linear' ? 'symbol-operator' : 'star',
    expandable: true,
    range: c.selectionRange ?? c.range ?? undefined,
    entity: { kind: 'constraint', name: c.name },
  };
}

function variableNode(v: VariableItem): ExplorerNode {
  const bounds = v.free ? 'free' : `[${formatBound(v.bounds.lower, '-inf')}, ${formatBound(v.bounds.upper, '+inf')}]`;
  const used = v.usage.objectives + v.usage.constraints;
  return {
    id: `variable:${v.name}`,
    kind: 'variable',
    label: v.name,
    description: v.unused ? 'unused' : `${bounds} · ${used}`,
    ...withDescription(
      `${v.kind}; ${v.usage.constraints} constraints, ${v.usage.objectives} objectives${v.usage.quadratic > 0 ? `, ${v.usage.quadratic} quadratic` : ''}`,
      v.description,
    ),
    icon: v.unused ? 'warning' : 'symbol-variable',
    expandable: !v.unused,
    range: v.range ?? undefined,
    entity: { kind: 'variable', name: v.name },
  };
}
