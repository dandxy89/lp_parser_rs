import { DiffChangeKind, DiffItem, DiffResult, DiffSectionName } from './protocol';

export interface DiffRow {
  readonly section: DiffSectionName;
  readonly change: DiffChangeKind;
  readonly name: string;
  readonly detail: string;
  /** `section.change` for `revealItem`; the host holds the ranges. */
  readonly group: string;
  readonly index: number;
  /** Whether the item has a range in the current buffer (removed items have none). */
  readonly navigable: boolean;
}

const SECTIONS: readonly DiffSectionName[] = ['objectives', 'constraints', 'variables'];
const CHANGES: readonly DiffChangeKind[] = ['added', 'removed', 'changed'];

/** The list `revealItem` addresses with `group` (`constraints.changed`). */
export function diffGroup(section: DiffSectionName, change: DiffChangeKind): string {
  return `${section}.${change}`;
}

export function diffItems(result: DiffResult, section: DiffSectionName, change: DiffChangeKind): readonly DiffItem[] {
  return result[section][change];
}

export function describeItem(change: DiffChangeKind, item: DiffItem): string {
  if (item.changes !== undefined && item.changes.length > 0) {
    return item.changes.join('; ');
  }
  if (item.old !== undefined && item.new !== undefined) {
    return `${item.old} → ${item.new}`;
  }
  return change === 'added' ? 'added' : change === 'removed' ? 'removed' : 'changed';
}

/** Flat, ordered rows for the table: objectives, constraints, variables; added, removed, changed. */
export function flattenDiff(result: DiffResult): DiffRow[] {
  const rows: DiffRow[] = [];
  for (const section of SECTIONS) {
    for (const change of CHANGES) {
      diffItems(result, section, change).forEach((item, index) => {
        rows.push({
          section,
          change,
          name: item.name,
          detail: describeItem(change, item),
          group: diffGroup(section, change),
          index,
          navigable: item.range !== undefined,
        });
      });
    }
  }
  return rows;
}

export interface DiffFilter {
  readonly text: string;
  readonly changes: ReadonlySet<DiffChangeKind>;
}

export function filterDiffRows(rows: readonly DiffRow[], filter: DiffFilter): DiffRow[] {
  const needle = filter.text.trim().toLowerCase();
  return rows.filter(
    (row) =>
      filter.changes.has(row.change) && (needle.length === 0 || row.name.toLowerCase().includes(needle) || row.detail.toLowerCase().includes(needle)),
  );
}

export function diffCounts(result: DiffResult): Record<DiffSectionName, Record<DiffChangeKind, number>> {
  const count = (section: DiffSectionName): Record<DiffChangeKind, number> => ({
    added: result[section].added.length,
    removed: result[section].removed.length,
    changed: result[section].changed.length,
  });
  return { objectives: count('objectives'), constraints: count('constraints'), variables: count('variables') };
}

/** One-line summary, e.g. `+2 -1 ~3 constraints, +1 variables`. */
export function diffSummary(result: DiffResult): string {
  if (result.identical) {
    return 'The models are identical.';
  }
  const counts = diffCounts(result);
  const parts: string[] = [];
  for (const section of SECTIONS) {
    const c = counts[section];
    const bits = [c.added > 0 ? `+${c.added}` : '', c.removed > 0 ? `-${c.removed}` : '', c.changed > 0 ? `~${c.changed}` : ''].filter(Boolean);
    if (bits.length > 0) {
      parts.push(`${bits.join(' ')} ${section}`);
    }
  }
  const sense = result.sense !== undefined ? `sense ${result.sense.old} → ${result.sense.new}` : '';
  return [sense, ...parts].filter(Boolean).join(', ') || 'No differences within tolerance.';
}
