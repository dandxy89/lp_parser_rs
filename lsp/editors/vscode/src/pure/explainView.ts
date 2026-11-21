import { ExplainResult } from './protocol';
import { formatNumber } from './solutionView';

/** `lower .. upper`, with an infinite side written as -inf / +inf. */
export function formatBounds(lower: number | null, upper: number | null): string {
  return `${lower === null ? '-inf' : formatNumber(lower)} .. ${upper === null ? '+inf' : formatNumber(upper)}`;
}

/** Plain-text facts about the entity: operator and right-hand side, activity range, bounds. */
export function explainFacts(result: ExplainResult): string[] {
  const facts: string[] = [];
  if (result.description !== undefined) {
    facts.push(`Description: ${result.description}`);
  }
  if (result.operator !== undefined && result.rhs !== undefined) {
    facts.push(`${result.operator} ${formatNumber(result.rhs)}`);
  }
  if (result.activity !== undefined) {
    facts.push(`Activity range: ${formatBounds(result.activity.min, result.activity.max)}`);
  }
  if (result.bounds !== undefined) {
    facts.push(`Bounds: ${formatBounds(result.bounds.lower, result.bounds.upper)} (${result.bounds.variableKind})`);
  }
  if (result.diagnostics.length > 0) {
    facts.push(`Diagnostics: ${result.diagnostics.join(', ')}`);
  }
  return facts;
}
