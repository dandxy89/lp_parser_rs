"""CLI for parse_lp: compare LP files and more."""

from __future__ import annotations

import argparse
import json
import sys
from typing import TYPE_CHECKING, Any

from parse_lp import LpParser

if TYPE_CHECKING:
    from parse_lp.parse_lp import StandardConstraint


def _parse_file(path: str) -> LpParser:
    """Parse an LP file, raising on missing file or parse error."""
    parser = LpParser(path)
    parser.parse()
    return parser


def _diff_objectives(p1: LpParser, p2: LpParser) -> list[dict[str, Any]]:
    """Return per-objective coefficient-level diffs."""
    objs1 = {o["name"]: {c["name"]: c["value"] for c in o["coefficients"]} for o in p1.objectives}
    objs2 = {o["name"]: {c["name"]: c["value"] for c in o["coefficients"]} for o in p2.objectives}
    details: list[dict[str, Any]] = []
    for name in sorted(set(objs1) | set(objs2)):
        if name not in objs1:
            details.append({"name": name, "status": "added"})
        elif name not in objs2:
            details.append({"name": name, "status": "removed"})
        else:
            changes = _diff_coefficients(objs1[name], objs2[name])
            if changes:
                details.append({"name": name, "status": "modified", "changes": changes})
    return details


def _diff_standard_constraints(c1: StandardConstraint, c2: StandardConstraint) -> list[dict[str, Any]]:
    """Return changes between two standard constraints."""
    coeffs1 = {c["name"]: c["value"] for c in c1["coefficients"]}
    coeffs2 = {c["name"]: c["value"] for c in c2["coefficients"]}
    changes = _diff_coefficients(coeffs1, coeffs2)
    if c1["rhs"] != c2["rhs"]:
        changes.append({"field": "rhs", "from": c1["rhs"], "to": c2["rhs"]})
    if c1["operator"] != c2["operator"]:
        changes.append({"field": "operator", "from": c1["operator"], "to": c2["operator"]})
    return changes


def _diff_constraints(p1: LpParser, p2: LpParser) -> list[dict[str, Any]]:
    """Return per-constraint coefficient/RHS diffs."""
    cons1 = {c["name"]: c for c in p1.constraints}
    cons2 = {c["name"]: c for c in p2.constraints}
    details: list[dict[str, Any]] = []
    for name in sorted(set(cons1) | set(cons2)):
        if name not in cons1:
            details.append({"name": name, "status": "added"})
        elif name not in cons2:
            details.append({"name": name, "status": "removed"})
        else:
            a, b = cons1[name], cons2[name]
            changes: list[dict[str, Any]] = []
            if a["type"] == "standard" and b["type"] == "standard":
                changes = _diff_standard_constraints(a, b)
            if changes:
                details.append({"name": name, "status": "modified", "changes": changes})
    return details


def _build_detailed_diff(p1: LpParser, p2: LpParser) -> dict[str, Any]:
    """Build a detailed diff dict comparing two parsed LP problems."""
    result = p1.compare(p2)
    return {
        "file1": p1.lp_file,
        "file2": p2.lp_file,
        "sense_changed": result["sense_changed"],
        "sense1": p1.sense,
        "sense2": p2.sense,
        "name_changed": result["name_changed"],
        "name1": p1.name,
        "name2": p2.name,
        "variable_count1": p1.variable_count(),
        "variable_count2": p2.variable_count(),
        "variable_count_diff": result["variable_count_diff"],
        "constraint_count1": p1.constraint_count(),
        "constraint_count2": p2.constraint_count(),
        "constraint_count_diff": result["constraint_count_diff"],
        "objective_count1": p1.objective_count(),
        "objective_count2": p2.objective_count(),
        "objective_count_diff": result["objective_count_diff"],
        "added_variables": result["added_variables"],
        "removed_variables": result["removed_variables"],
        "modified_variables": result["modified_variables"],
        "added_constraints": result["added_constraints"],
        "removed_constraints": result["removed_constraints"],
        "objective_details": _diff_objectives(p1, p2),
        "constraint_details": _diff_constraints(p1, p2),
    }


def _diff_coefficients(coeffs1: dict[str, float], coeffs2: dict[str, float]) -> list[dict[str, Any]]:
    """Diff two coefficient dicts, returning a list of changes."""
    changes: list[dict[str, Any]] = []
    for var in sorted(set(coeffs1) | set(coeffs2)):
        if var not in coeffs1:
            changes.append({"variable": var, "status": "added", "value": coeffs2[var]})
        elif var not in coeffs2:
            changes.append({"variable": var, "status": "removed", "value": coeffs1[var]})
        elif coeffs1[var] != coeffs2[var]:
            changes.append({"variable": var, "status": "modified", "from": coeffs1[var], "to": coeffs2[var]})
    return changes


def _format_coeff_changes(changes: list[dict[str, Any]]) -> list[str]:
    """Format coefficient changes as human-readable lines."""
    lines: list[str] = []
    for ch in changes:
        if ch.get("field"):
            lines.append(f"      {ch['field']}: {ch['from']} -> {ch['to']}")
        elif ch["status"] == "added":
            lines.append(f"      + {ch['variable']}: {ch['value']}")
        elif ch["status"] == "removed":
            lines.append(f"      - {ch['variable']}: {ch['value']}")
        else:
            lines.append(f"      ~ {ch['variable']}: {ch['from']} -> {ch['to']}")
    return lines


def _print_summary(diff: dict[str, Any]) -> None:
    """Print the summary section of a human-readable diff."""
    print("Summary")
    print("-------")
    if diff["sense_changed"]:
        print(f"  Sense: {diff['sense1']} -> {diff['sense2']}")
    if diff["name_changed"]:
        print(f"  Name: {diff['name1']} -> {diff['name2']}")
    for label, key1, key2, dkey in [
        ("Variables", "variable_count1", "variable_count2", "variable_count_diff"),
        ("Constraints", "constraint_count1", "constraint_count2", "constraint_count_diff"),
        ("Objectives", "objective_count1", "objective_count2", "objective_count_diff"),
    ]:
        d = diff[dkey]
        suffix = f" ({d:+d})" if d != 0 else ""
        print(f"  {label}: {diff[key1]} -> {diff[key2]}{suffix}")


def _print_variables(diff: dict[str, Any]) -> None:
    """Print the variables section if there are changes."""
    added = diff["added_variables"]
    removed = diff["removed_variables"]
    modified = diff["modified_variables"]
    if not (added or removed or modified):
        return
    print()
    print("Variables")
    print("---------")
    for v in added:
        print(f"  + {v}")
    for v in removed:
        print(f"  - {v}")
    for v in modified:
        print(f"  ~ {v}")


def _print_detail_section(title: str, underline: str, items: list[dict[str, Any]]) -> None:
    """Print an objective or constraint detail section."""
    if not items:
        return
    print()
    print(title)
    print(underline)
    for item in items:
        if item["status"] == "added":
            print(f"  + {item['name']}")
        elif item["status"] == "removed":
            print(f"  - {item['name']}")
        else:
            print(f"  ~ {item['name']}:")
            for line in _format_coeff_changes(item["changes"]):
                print(line)


def _print_human_diff(diff: dict[str, Any], *, summary_only: bool = False) -> None:
    """Print a human-readable diff to stdout."""
    print(f"--- {diff['file1']}")
    print(f"+++ {diff['file2']}")
    print()
    _print_summary(diff)
    _print_variables(diff)
    if not summary_only:
        _print_detail_section("Objectives", "----------", diff["objective_details"])
        _print_detail_section("Constraint Details", "------------------", diff["constraint_details"])


def _is_identical(diff: dict[str, Any]) -> bool:
    """Check whether a diff represents identical files."""
    return (
        not diff["sense_changed"]
        and not diff["name_changed"]
        and diff["variable_count_diff"] == 0
        and diff["constraint_count_diff"] == 0
        and diff["objective_count_diff"] == 0
        and not diff["added_variables"]
        and not diff["removed_variables"]
        and not diff["modified_variables"]
        and not diff["added_constraints"]
        and not diff["removed_constraints"]
        and not diff["objective_details"]
        and not diff["constraint_details"]
    )


def _cmd_diff(args: argparse.Namespace) -> int:
    """Execute the diff subcommand."""
    try:
        p1 = _parse_file(args.file1)
        p2 = _parse_file(args.file2)
    except FileExistsError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 2
    except RuntimeError as exc:
        print(f"Parse error: {exc}", file=sys.stderr)
        return 2

    diff = _build_detailed_diff(p1, p2)
    identical = _is_identical(diff)

    if args.json:
        if not args.quiet:
            diff["identical"] = identical
            print(json.dumps(diff, indent=2))
        return 0 if identical else 1

    if args.quiet:
        return 0 if identical else 1

    if identical:
        print(f"Files are identical: {args.file1}")
        return 0

    _print_human_diff(diff, summary_only=args.summary)
    return 1


def main(argv: list[str] | None = None) -> int:
    """CLI entry point. Returns exit code (0=identical, 1=different, 2=error)."""
    parser = argparse.ArgumentParser(prog="parse-lp", description="LP file utilities")
    subparsers = parser.add_subparsers(dest="command")

    diff_parser = subparsers.add_parser("diff", help="Compare two LP files")
    diff_parser.add_argument("file1", help="First LP file")
    diff_parser.add_argument("file2", help="Second LP file")
    diff_parser.add_argument("--json", action="store_true", help="Output as JSON")
    diff_parser.add_argument("-q", "--quiet", action="store_true", help="No output, exit code only")
    diff_parser.add_argument("--summary", action="store_true", help="Skip detailed coefficient diffs")

    args = parser.parse_args(argv)

    if args.command is None:
        parser.print_help()
        return 2

    if args.command == "diff":
        return _cmd_diff(args)

    return 2


def cli_entry() -> None:
    """Wrapper for console_scripts entry point."""
    sys.exit(main())
