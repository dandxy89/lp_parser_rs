import type { LspRange } from './modelTypes';

/** A published diagnostic reduced to what the analysis view needs. */
export interface AnalysisDiagnostic {
  readonly code: string;
  readonly message: string;
  /** LSP severity: 1 error, 2 warning, 3 information, 4 hint. */
  readonly severity: 1 | 2 | 3 | 4;
  /** From the diagnostic's `data.category`; `undefined` when the server did not supply one. */
  readonly category: string | undefined;
  readonly certainty: string | undefined;
  readonly range: LspRange;
}

export interface AnalysisNode {
  readonly id: string;
  readonly kind: 'category' | 'code' | 'finding';
  readonly label: string;
  readonly description?: string;
  readonly icon: string;
  readonly children: readonly AnalysisNode[];
  readonly range?: LspRange;
  readonly severity?: 1 | 2 | 3 | 4;
}

export const CATEGORY_ORDER = ['structural', 'numerical', 'mathematical', 'performance', 'solver'] as const;
export const UNCATEGORISED = 'uncategorised';

const CATEGORY_LABEL: Readonly<Record<string, string>> = {
  structural: 'Structural',
  numerical: 'Numerical',
  mathematical: 'Mathematical',
  performance: 'Performance',
  solver: 'Solver',
  [UNCATEGORISED]: 'Other',
};

const CATEGORY_ICON: Readonly<Record<string, string>> = {
  structural: 'symbol-structure',
  numerical: 'symbol-numeric',
  mathematical: 'symbol-operator',
  performance: 'dashboard',
  solver: 'beaker',
  [UNCATEGORISED]: 'circle-outline',
};

function severityIcon(severity: 1 | 2 | 3 | 4): string {
  return severity === 1 ? 'error' : severity === 2 ? 'warning' : severity === 3 ? 'info' : 'lightbulb';
}

function categoryRank(category: string): number {
  if (category === UNCATEGORISED) {
    return Number.MAX_SAFE_INTEGER;
  }
  const index = (CATEGORY_ORDER as readonly string[]).indexOf(category);
  return index === -1 ? CATEGORY_ORDER.length : index;
}

/**
 * Groups diagnostics by category, then by code. Categories follow the fixed order above, unknown categories
 * sort last alphabetically; codes sort by worst severity, then name; findings sort by position.
 */
export function groupByCategory(diagnostics: readonly AnalysisDiagnostic[]): AnalysisNode[] {
  const byCategory = new Map<string, Map<string, AnalysisDiagnostic[]>>();
  for (const diagnostic of diagnostics) {
    const category = diagnostic.category ?? UNCATEGORISED;
    let codes = byCategory.get(category);
    if (codes === undefined) {
      codes = new Map();
      byCategory.set(category, codes);
    }
    const list = codes.get(diagnostic.code);
    if (list === undefined) {
      codes.set(diagnostic.code, [diagnostic]);
    } else {
      list.push(diagnostic);
    }
  }

  const categories = [...byCategory.keys()].sort((a, b) => categoryRank(a) - categoryRank(b) || a.localeCompare(b));
  return categories.map((category): AnalysisNode => {
    const codes = byCategory.get(category) ?? new Map<string, AnalysisDiagnostic[]>();
    const codeNodes = [...codes.entries()]
      .map(([code, findings]): AnalysisNode => {
        const worst = Math.min(...findings.map((f) => f.severity)) as 1 | 2 | 3 | 4;
        const sorted = [...findings].sort(
          (a, b) => a.range.start.line - b.range.start.line || a.range.start.character - b.range.start.character,
        );
        return {
          id: `code:${category}:${code}`,
          kind: 'code',
          label: code,
          description: String(findings.length),
          icon: severityIcon(worst),
          severity: worst,
          children: sorted.map((f, index) => ({
            id: `finding:${category}:${code}:${index}`,
            kind: 'finding' as const,
            label: f.message,
            description: f.certainty,
            icon: severityIcon(f.severity),
            severity: f.severity,
            range: f.range,
            children: [],
          })),
        };
      })
      .sort((a, b) => (a.severity ?? 4) - (b.severity ?? 4) || a.label.localeCompare(b.label));
    const total = codeNodes.reduce((sum, node) => sum + node.children.length, 0);
    return {
      id: `category:${category}`,
      kind: 'category',
      label: CATEGORY_LABEL[category] ?? category,
      description: String(total),
      icon: CATEGORY_ICON[category] ?? CATEGORY_ICON[UNCATEGORISED],
      children: codeNodes,
    };
  });
}

/** Reads `data.category` / `data.certainty` from a diagnostic's opaque `data`; anything else yields `undefined`. */
export function readDiagnosticData(data: unknown): { category: string | undefined; certainty: string | undefined } {
  if (typeof data !== 'object' || data === null) {
    return { category: undefined, certainty: undefined };
  }
  const record = data as Record<string, unknown>;
  return {
    category: typeof record.category === 'string' ? record.category : undefined,
    certainty: typeof record.certainty === 'string' ? record.certainty : undefined,
  };
}
