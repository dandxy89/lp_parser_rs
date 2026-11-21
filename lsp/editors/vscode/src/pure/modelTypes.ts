/** Wire types for `lp/model` and `lp/explain` (fleet contract 2.2 and 2.3). Only the fields the extension reads are declared. */

export interface LspPosition {
  readonly line: number;
  readonly character: number;
}

export interface LspRange {
  readonly start: LspPosition;
  readonly end: LspPosition;
}

export interface ModelSummary {
  readonly name: string | null;
  readonly sense: string;
  readonly objectives: number;
  readonly constraints: number;
  readonly variables: number;
  readonly nonzeros: number;
  readonly density: number;
  readonly variablesByKind: Readonly<Record<string, number>>;
  readonly constraintsByKind: Readonly<Record<string, number>>;
  readonly diagnostics: { readonly error: number; readonly warning: number; readonly info: number };
}

export interface ObjectiveItem {
  readonly name: string;
  readonly range: LspRange | null;
  readonly selectionRange: LspRange | null;
  readonly terms: number;
  readonly quadraticTerms: number;
}

export interface ConstraintItem {
  readonly name: string;
  readonly generated: boolean;
  /** `linear`, `indicator`, `quadratic`, `sos1`, `sos2` or `general`. */
  readonly kind: string;
  /** `normal`, `lazy` or `userCut`. */
  readonly class: string;
  readonly operator?: string;
  readonly rhs?: number | null;
  readonly terms: number;
  readonly rangePartner: string | null;
  readonly range: LspRange | null;
  readonly selectionRange: LspRange | null;
}

export interface VariableItem {
  readonly name: string;
  /** `continuous`, `integer`, `general`, `binary`, `semiContinuous`, `semiInteger` or `sos`. */
  readonly kind: string;
  readonly bounds: {
    readonly lower: number | null;
    readonly upper: number | null;
  };
  readonly usage: { readonly objectives: number; readonly constraints: number; readonly quadratic: number };
  readonly unused: boolean;
  readonly free: boolean;
  readonly range: LspRange | null;
}

export interface ModelResult {
  readonly uri: string;
  readonly version: number | null;
  readonly summary?: ModelSummary;
  readonly objectives?: readonly ObjectiveItem[];
  readonly constraints?: readonly ConstraintItem[];
  readonly variables?: readonly VariableItem[];
  readonly truncated: { readonly objectives: boolean; readonly constraints: boolean; readonly variables: boolean };
}

export type EntityKind = 'constraint' | 'objective' | 'variable';

export interface ExplainTerm {
  readonly variable: string;
  readonly coefficient: number | null;
  readonly range: LspRange | null;
}

export interface ExplainUsage {
  readonly entity: string | null;
  readonly kind: string | null;
  readonly role: string;
  readonly coefficient: number | null;
  readonly range: LspRange;
}

export interface ExplainResult {
  readonly uri: string;
  readonly version: number | null;
  readonly kind: EntityKind;
  readonly name: string;
  readonly range: LspRange | null;
  readonly markdown: string;
  readonly terms?: readonly ExplainTerm[];
  readonly termsTruncated?: boolean;
  readonly usage?: readonly ExplainUsage[];
  readonly usageTruncated?: boolean;
}

/** The two requests the Model Explorer needs; the extension implements it over the language client, tests with a mock. */
export interface ModelClient {
  model(uri: string, include?: readonly ('summary' | 'objectives' | 'constraints' | 'variables')[]): Promise<ModelResult>;
  explain(uri: string, target: { readonly kind: EntityKind; readonly name: string }): Promise<ExplainResult>;
}
