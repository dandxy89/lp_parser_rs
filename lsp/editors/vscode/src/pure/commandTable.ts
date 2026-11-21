import type { LpRequest } from './capabilities';

/** A command that drives one custom LSP request. */
export interface FeatureCommand {
  readonly command: string;
  readonly title: string;
  /** The server request the command calls; the command is enabled only when it is advertised. */
  readonly request: LpRequest;
}

export const FEATURE_COMMANDS: readonly FeatureCommand[] = [
  { command: 'lp.solve', title: 'Solve Model', request: 'lp/solve' },
  { command: 'lp.showMatrix', title: 'Show Coefficient Matrix', request: 'lp/matrix' },
  { command: 'lp.showDependencyGraph', title: 'Show Dependency Graph', request: 'lp/graph' },
  { command: 'lp.compareModels', title: 'Compare Models', request: 'lp/diff' },
  { command: 'lp.explainConstraint', title: 'Explain Constraint', request: 'lp/explain' },
  { command: 'lp.explainVariable', title: 'Explain Variable', request: 'lp/explain' },
  { command: 'lp.diagnoseInfeasibility', title: 'Diagnose Infeasibility', request: 'lp/solve' },
];
