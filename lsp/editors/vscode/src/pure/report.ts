import { AnalysisDiagnostic, groupByCategory } from './analysis';
import type { ModelResult } from './modelTypes';
import { isRecord } from './sanitise';

export interface ReportInput {
  /** Basename for the title. */
  readonly fileName: string;
  /** The raw `lp/model` wire result: treated as untrusted. */
  readonly model: ModelResult;
  readonly diagnostics: readonly AnalysisDiagnostic[];
}

const MISSING = '–';
const NOT_REPORTED = 'Not reported by the server.';

/** Escapes Markdown syntax (and `|`, so table cells hold) and flattens line breaks; no raw HTML or link survives. */
export function escapeMarkdown(text: string): string {
  return text
    .replace(/[\r\n]+/g, ' ')
    .replace(/&/g, '&amp;')
    .replace(/[\\`*_{}[\]()#+\-.!|<>~]/g, '\\$&');
}

/** "model.lp" -> "model.report.md"; "x.LP" -> "x.report.md"; "noext" -> "noext.report.md". */
export function reportFileName(sourceBaseName: string): string {
  return `${sourceBaseName.replace(/\.lp$/i, '')}.report.md`;
}

function str(value: unknown): string {
  return typeof value === 'string' ? escapeMarkdown(value) : MISSING;
}

function num(value: unknown): string {
  return typeof value === 'number' && Number.isFinite(value) ? String(value) : MISSING;
}

/** The optional `@description` text of a result, objective, constraint or variable; read structurally. */
function descriptionOf(item: unknown): string | undefined {
  if (!isRecord(item)) {
    return undefined;
  }
  return typeof item.description === 'string' && item.description !== '' ? item.description : undefined;
}

function records(value: unknown): Record<string, unknown>[] | undefined {
  return Array.isArray(value) ? value.filter(isRecord) : undefined;
}

function table(headers: readonly string[], rows: readonly (readonly string[])[]): string {
  const line = (cells: readonly string[]): string => `| ${cells.join(' | ')} |`;
  return [line(headers), line(headers.map(() => '---')), ...rows.map(line)].join('\n');
}

/** A list section: a table with a Description column only when some entry has one. */
function listSection(
  items: Record<string, unknown>[] | undefined,
  headers: readonly string[],
  row: (item: Record<string, unknown>) => string[],
): string {
  if (items === undefined) {
    return NOT_REPORTED;
  }
  if (items.length === 0) {
    return 'None.';
  }
  const withDescription = items.some((item) => descriptionOf(item) !== undefined);
  return table(
    withDescription ? [...headers, 'Description'] : headers,
    items.map((item) => (withDescription ? [...row(item), escapeMarkdown(descriptionOf(item) ?? '')] : row(item))),
  );
}

function statistics(summary: unknown, truncated: unknown): string {
  if (!isRecord(summary)) {
    return NOT_REPORTED;
  }
  const counts = isRecord(summary.diagnostics) ? summary.diagnostics : {};
  const density = typeof summary.density === 'number' && Number.isFinite(summary.density) ? `${(summary.density * 100).toFixed(2)}%` : MISSING;
  const lines = [
    table(
      ['Statistic', 'Value'],
      [
        ['Sense', str(summary.sense)],
        ['Objectives', num(summary.objectives)],
        ['Constraints', num(summary.constraints)],
        ['Variables', num(summary.variables)],
        ['Non-zeros', num(summary.nonzeros)],
        ['Density', density],
        ['Errors', num(counts.error)],
        ['Warnings', num(counts.warning)],
        ['Information', num(counts.info)],
      ],
    ),
  ];
  if (isRecord(truncated) && Object.values(truncated).some((flag) => flag === true)) {
    lines.push('> Lists are truncated by the server; the counts above are complete.');
  }
  return lines.join('\n\n');
}

function bound(value: unknown, infinity: string): string {
  return value === null ? infinity : num(value);
}

function bounds(variables: Record<string, unknown>[] | undefined): string {
  if (variables === undefined) {
    return NOT_REPORTED;
  }
  const rows = variables
    .filter((variable) => isRecord(variable.bounds) && !(variable.bounds.lower === 0 && variable.bounds.upper === null))
    .map((variable) => {
      const limits = isRecord(variable.bounds) ? variable.bounds : {};
      return [str(variable.name), bound(limits.lower, '-inf'), bound(limits.upper, '+inf')];
    });
  return rows.length === 0 ? 'All variables use the default bounds [0, +inf).' : table(['Variable', 'Lower', 'Upper'], rows);
}

function variableTypes(summary: unknown, variables: Record<string, unknown>[] | undefined): string {
  const lines: string[] = [];
  const kinds = isRecord(summary) && isRecord(summary.variablesByKind) ? summary.variablesByKind : undefined;
  if (kinds === undefined) {
    lines.push(NOT_REPORTED);
  } else {
    const rows = Object.keys(kinds)
      .sort()
      .filter((kind) => typeof kinds[kind] === 'number' && Number.isFinite(kinds[kind]) && kinds[kind] !== 0)
      .map((kind) => [escapeMarkdown(kind), num(kinds[kind])]);
    lines.push(rows.length === 0 ? 'None.' : table(['Type', 'Count'], rows));
  }
  lines.push(`Unused variables: ${variables === undefined ? MISSING : variables.filter((variable) => variable.unused === true).length}`);
  const described = (variables ?? []).filter((variable) => descriptionOf(variable) !== undefined);
  if (described.length > 0) {
    lines.push(table(['Variable', 'Type', 'Description'], described.map((v) => [str(v.name), str(v.kind), escapeMarkdown(descriptionOf(v) ?? '')])));
  }
  return lines.join('\n\n');
}

function warnings(diagnostics: readonly AnalysisDiagnostic[]): string {
  const categories = groupByCategory(diagnostics);
  if (categories.length === 0) {
    return 'No findings.';
  }
  const lines: string[] = [];
  for (const category of categories) {
    lines.push(`### ${escapeMarkdown(category.label)} (${category.description ?? 0})`, '');
    for (const code of category.children) {
      lines.push(`- **${escapeMarkdown(code.label)}** (${code.description ?? 0})`);
      for (const finding of code.children) {
        const line = finding.range === undefined ? MISSING : String(finding.range.start.line + 1);
        const certainty = finding.description === undefined ? '' : ` (${escapeMarkdown(finding.description)})`;
        lines.push(`  - Line ${line}: ${escapeMarkdown(finding.label)}${certainty}`);
      }
    }
    lines.push('');
  }
  return lines.join('\n').trimEnd();
}

/** Renders the Markdown model report. Deterministic: no timestamp. */
export function renderModelReport(input: ReportInput): string {
  const model: unknown = input.model;
  const result = isRecord(model) ? model : {};
  const summary = result.summary;
  const variables = records(result.variables);
  const head = [`# Model report: ${escapeMarkdown(input.fileName)}`];
  if (isRecord(summary) && typeof summary.name === 'string') {
    head.push(`Model name: ${escapeMarkdown(summary.name)}`);
  }
  const description = descriptionOf(result);
  if (description !== undefined) {
    head.push(escapeMarkdown(description));
  }
  const sections: [string, string][] = [
    ['Statistics', statistics(summary, result.truncated)],
    ['Objectives', listSection(records(result.objectives), ['Name', 'Terms'], (o) => [str(o.name), num(o.terms)])],
    [
      'Constraints',
      listSection(records(result.constraints), ['Name', 'Kind', 'Class', 'Sense', 'RHS', 'Terms'], (c) => [
        `${str(c.name)}${c.generated === true ? ' (generated)' : ''}`,
        str(c.kind),
        str(c.class),
        str(c.operator),
        num(c.rhs),
        num(c.terms),
      ]),
    ],
    ['Bounds', bounds(variables)],
    ['Variable types', variableTypes(summary, variables)],
    ['Warnings', warnings(input.diagnostics)],
  ];
  return `${[head.join('\n\n'), ...sections.map(([title, body]) => `## ${title}\n\n${body}`)].join('\n\n')}\n`;
}
