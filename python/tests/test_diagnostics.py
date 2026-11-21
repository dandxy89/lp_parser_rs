from __future__ import annotations

import pytest

from parse_lp import LpInvalidValueError, LpParser

CODES = {
    "invalid-bounds",
    "duplicate-row",
    "proportional-row",
    "contradictory-rows",
    "no-effective-bound",
    "row-infeasible-by-bounds",
}


def diagnose(text: str, config: dict | None = None) -> list[dict]:
    return LpParser.from_string(text).diagnostics(config)


def codes(findings: list[dict]) -> list[str]:
    return [finding["code"] for finding in findings]


class TestShape:
    LP = "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 2\n c2: 2 x + 2 y >= 4\nEnd\n"

    def test_returns_list_of_dicts_with_documented_keys(self) -> None:
        findings = diagnose(self.LP)
        assert findings
        for finding in findings:
            assert set(finding) == {
                "code",
                "severity",
                "category",
                "certainty",
                "message",
                "explanation",
                "subject",
                "related",
            }
            assert finding["severity"] in {"error", "warning", "info"}
            assert finding["category"] in {"structural", "numerical", "mathematical", "performance"}
            assert finding["certainty"] in {"proven", "heuristic"}

    def test_subject_and_related_shape(self) -> None:
        (finding,) = [f for f in diagnose(self.LP) if f["code"] == "proportional-row"]
        assert finding["subject"] == {"kind": "constraint", "name": "c2", "variable": None, "site": "definition"}
        assert finding["certainty"] == "proven"
        assert finding["severity"] == "warning"
        assert finding["related"] == [
            {
                "kind": "constraint",
                "name": "c1",
                "variable": None,
                "site": "definition",
                "note": "the row it is proportional to",
            }
        ]
        assert "scaled by 2" in finding["message"]

    def test_problem_wide_finding_has_no_subject(self) -> None:
        lp = "Minimize\n obj: 1e12 x + y\nSubject To\n c1: x + y >= 1\nEnd\n"
        (finding,) = [f for f in diagnose(lp) if f["code"] == "coefficient-range"]
        assert finding["subject"] is None
        assert finding["related"] == []
        assert finding["certainty"] == "heuristic"

    def test_sorted_by_severity(self) -> None:
        lp = "Minimize\n obj: x\nSubject To\n c1: x >= 100\n c2: x + y >= 1\nBounds\n x <= 50\n z free\nEnd\n"
        rank = {"error": 0, "warning": 1, "info": 2}
        findings = diagnose(lp)
        assert [rank[f["severity"]] for f in findings] == sorted(rank[f["severity"]] for f in findings)
        assert findings[0]["code"] == "row-infeasible-by-bounds"

    def test_deterministic(self) -> None:
        assert diagnose(self.LP) == diagnose(self.LP)

    def test_clean_model_has_no_errors(self) -> None:
        lp = "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 2\n c2: x - y <= 1\nBounds\n x <= 10\n y <= 10\nEnd\n"
        assert not [f for f in diagnose(lp) if f["severity"] in {"error", "warning"}]


class TestChecks:
    def test_invalid_bounds_subject_site(self) -> None:
        lp = "Minimize\n obj: x\nSubject To\n c1: x >= 0\nBounds\n x >= 5\n x <= 3\nEnd\n"
        (finding,) = [f for f in diagnose(lp) if f["code"] == "invalid-bounds"]
        assert finding["subject"]["site"] == "bound"
        assert finding["subject"]["kind"] == "variable"
        assert finding["severity"] == "error"

    def test_no_effective_bound_is_conditional(self) -> None:
        lp = "Maximize\n obj: x + w\nSubject To\n c1: x <= 1\nEnd\n"
        (finding,) = [f for f in diagnose(lp) if f["code"] == "no-effective-bound"]
        assert finding["subject"]["name"] == "w"
        assert "if the model is feasible" in finding["message"]

    def test_contradictory_rows(self) -> None:
        lp = "Minimize\n obj: x + y\nSubject To\n c1: x + y <= 1\n c2: x + y >= 5\nEnd\n"
        (finding,) = [f for f in diagnose(lp) if f["code"] == "contradictory-rows"]
        assert finding["subject"]["name"] == "c2"
        assert finding["related"][0]["name"] == "c1"

    def test_coefficient_level_subject_names_variable(self) -> None:
        lp = "Minimize\n obj: x\nSubject To\n c1: 1e12 x + y >= 1\nEnd\n"
        (finding,) = [f for f in diagnose(lp) if f["code"] == "large-coefficient"]
        assert finding["subject"] == {"kind": "constraint", "name": "c1", "variable": "x", "site": "definition"}

    def test_objective_subject_kind(self) -> None:
        lp = "Minimize multi-objectives\n a: x + y\n b: 2 x + 2 y\nSubject To\n c1: x + y >= 1\nEnd\n"
        (finding,) = [f for f in diagnose(lp) if f["code"] == "duplicate-objective"]
        assert finding["subject"]["kind"] == "objective"
        assert finding["subject"]["name"] == "b"

    def test_mps_input(self) -> None:
        mps = (
            "NAME          t\nROWS\n N  COST\n L  R1\n L  R2\nCOLUMNS\n"
            "    X         COST      1.0   R1        1.0\n    X         R2        1.0\n"
            "RHS\n    RHS       R1        4.0   R2        4.0\nENDATA\n"
        )
        findings = LpParser.from_string(mps, "mps").diagnostics()
        assert "duplicate-row" in codes(findings)

    def test_all_codes_are_known_strings(self) -> None:
        lp = "Minimize\n obj: x\nSubject To\n c1: x >= 100\n c2: x >= 100\nBounds\n x <= 50\nEnd\n"
        assert set(codes(diagnose(lp))) <= {
            *CODES,
            "unused-variable",
            "singleton-row",
            "fixed-variable",
            "dominated-row",
            "rows-form-range",
            "row-redundant-by-bounds",
        }


class TestConfig:
    LP = "Minimize\n obj: x\nSubject To\n c1: 5 x >= 1\nEnd\n"

    def test_threshold_is_applied(self) -> None:
        assert "large-coefficient" not in codes(diagnose(self.LP))
        findings = diagnose(self.LP, {"thresholds": {"largeCoefficientThreshold": 2.0}})
        assert "large-coefficient" in codes(findings)

    def test_max_per_code_truncates(self) -> None:
        rows = "".join(f" c{i}: 0 x >= 1\n" for i in range(5))
        lp = f"Minimize\n obj: x\nSubject To\n{rows}End\n"
        findings = diagnose(lp, {"maxPerCode": 2})
        assert codes(findings).count("empty-row-infeasible") == 2
        (notice,) = [
            f for f in findings if f["code"] == "diagnostics-truncated" and "empty-row-infeasible" in f["message"]
        ]
        assert notice["message"].startswith("3 further")
        assert notice["subject"] is None

    @pytest.mark.parametrize(
        "config",
        [
            {"feasibilityTolerance": -1.0},
            {"equalityTolerance": float("nan")},
            {"rhsCoefficientRatioThreshold": float("inf")},
            {"maxPerCode": 0},
            {"thresholds": {"largeRhsThreshold": -1.0}},
        ],
    )
    def test_invalid_values_raise(self, config: dict) -> None:
        with pytest.raises(LpInvalidValueError):
            diagnose(self.LP, config)

    @pytest.mark.parametrize("config", [{"nope": 1}, {"thresholds": {"bogus": 1.0}}, {"thresholds": 3}])
    def test_unknown_keys_raise(self, config: dict) -> None:
        with pytest.raises(LpInvalidValueError):
            diagnose(self.LP, config)

    def test_wrong_value_type_raises(self) -> None:
        with pytest.raises(LpInvalidValueError):
            diagnose(self.LP, {"maxPerCode": "many"})

    def test_empty_config_equals_default(self) -> None:
        assert diagnose(self.LP, {}) == diagnose(self.LP)
