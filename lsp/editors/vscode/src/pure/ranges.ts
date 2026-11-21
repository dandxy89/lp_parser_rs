import { DiffChangeKind, DiffResult, DiffSectionName, ExplainResult, SolveResult } from './protocol';
import { LspRange } from './sanitise';

const SECTIONS: readonly string[] = ['variables', 'constraints', 'objectives'];
const CHANGES: readonly string[] = ['added', 'removed', 'changed'];

/** Source range of a diff item addressed by `group` (`constraints.changed`) and index. Removed items have none. */
export function diffRange(result: DiffResult, group: string, index: number): LspRange | undefined {
  const [section, change] = group.split('.');
  if (!SECTIONS.includes(section) || !CHANGES.includes(change)) {
    return undefined;
  }
  return result[section as DiffSectionName][change as DiffChangeKind][index]?.range;
}

/** Source range for a solution table row or infeasibility node. */
export function solutionRange(result: SolveResult, group: string, index: number): LspRange | undefined {
  switch (group) {
    case 'variables':
      return result.variables[index]?.range;
    case 'constraints':
      return result.constraints[index]?.range;
    case 'infeasibility.constraints':
      return result.infeasibility?.constraints[index]?.range;
    case 'infeasibility.bounds':
      return result.infeasibility?.bounds[index]?.range;
    default:
      return undefined;
  }
}

/** The explained entity's own range (`explain`, index 0). */
export function explainRange(result: ExplainResult, group: string, index: number): LspRange | undefined {
  return group === 'explain' && index === 0 ? result.range : undefined;
}
