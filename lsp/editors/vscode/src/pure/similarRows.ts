import { LspRange, cleanText } from './sanitise';

/** Model-analysis codes (`rust/src/analysis/rows.rs`) that name two similar rows. */
export const SIMILAR_CODES = ['duplicate-row', 'proportional-row'] as const;
export type SimilarCode = (typeof SIMILAR_CODES)[number];

export interface SimilarDiagnostic {
  readonly source: string | undefined;
  readonly code: string | undefined;
  readonly message: string;
  readonly range: LspRange;
  /** Ranges (in the same document) the diagnostic points to through `relatedInformation`. */
  readonly related: readonly LspRange[];
}

export interface SimilarFinding {
  readonly code: SimilarCode;
  /** The later row, where the diagnostic is reported. */
  readonly row: LspRange;
  /** The row it duplicates or is proportional to. */
  readonly partner: LspRange;
  readonly message: string;
}

export interface SimilarPick {
  readonly label: string;
  readonly description: string;
  readonly detail: string;
  readonly target: LspRange;
}

const SOURCE = 'lp-lsp';

function isSimilarCode(code: string | undefined): code is SimilarCode {
  return (SIMILAR_CODES as readonly (string | undefined)[]).includes(code);
}

/**
 * Keeps the server's duplicate and proportional row findings. A `diagnostics-truncated` note that names one of the
 * codes becomes a plain note, because the dropped findings cannot be navigated to.
 */
export function similarFindings(diagnostics: readonly SimilarDiagnostic[]): { findings: SimilarFinding[]; notes: string[] } {
  const findings: SimilarFinding[] = [];
  const notes: string[] = [];
  for (const d of diagnostics) {
    if (d.source !== SOURCE) {
      continue;
    }
    if (isSimilarCode(d.code)) {
      if (d.related.length > 0) {
        findings.push({ code: d.code, row: d.range, partner: d.related[0], message: d.message });
      }
    } else if (d.code === 'diagnostics-truncated' && SIMILAR_CODES.some((code) => d.message.includes(`'${code}'`))) {
      notes.push(cleanText(d.message, 200));
    }
  }
  return { findings, notes };
}

function compare(a: { line: number; character: number }, b: { line: number; character: number }): number {
  return a.line - b.line || a.character - b.character;
}

function startsIn(range: LspRange, focus: LspRange): boolean {
  return compare(range.start, focus.start) >= 0 && compare(range.start, focus.end) <= 0;
}

/** Findings whose row or partner starts inside `focus` (inclusive). */
export function findingsFor(findings: readonly SimilarFinding[], focus: LspRange): SimilarFinding[] {
  return findings.filter((f) => startsIn(f.row, focus) || startsIn(f.partner, focus));
}

/** Collapses whitespace, cleans and cuts to 80, and escapes codicons (`$(` becomes `\$(`). */
export function rowLabel(text: string): string {
  return cleanText(text.replace(/\s+/g, ' ').trim(), 80).replace(/\$\(/g, '\\$(');
}

/**
 * One pick per (target, other) pair, sorted by the target's position. The target is the side outside `focus`
 * (the row itself when there is no focus); the description names the other side.
 */
export function similarPicks(findings: readonly SimilarFinding[], nameAt: (range: LspRange) => string, focus?: LspRange): SimilarPick[] {
  const seen = new Set<string>();
  const picks: { pick: SimilarPick; other: LspRange }[] = [];
  for (const f of findings) {
    const rowFocused = focus !== undefined && startsIn(f.row, focus);
    const [target, other] = rowFocused ? [f.partner, f.row] : [f.row, f.partner];
    const label = rowLabel(nameAt(target));
    const description = `${f.code === 'duplicate-row' ? 'duplicate of' : 'proportional to'} ${rowLabel(nameAt(other))}`;
    const key = `${target.start.line}:${target.start.character}|${description}`;
    if (!seen.has(key)) {
      seen.add(key);
      picks.push({ pick: { label, description, detail: cleanText(f.message.split('\n')[0], 200), target }, other });
    }
  }
  picks.sort((a, b) => compare(a.pick.target.start, b.pick.target.start) || compare(a.other.start, b.other.start));
  return picks.map((p) => p.pick);
}
