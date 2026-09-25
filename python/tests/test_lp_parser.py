from __future__ import annotations

import errno
from typing import TYPE_CHECKING

import pytest

from parse_lp import LpInvalidValueError, LpObjectNotFoundError, LpParseError, LpParser

from .conftest import EXPECTED_PARSE_FAILURES, collect_lp_resource_files

if TYPE_CHECKING:
    from collections.abc import Callable
    from contextlib import AbstractContextManager
    from pathlib import Path


class TestLpParserBasic:
    def test_create_parser(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        assert parser.lp_file == str(simple_lp_file)

    def test_class_module(self) -> None:
        assert LpParser.__module__ == "parse_lp"

    def test_repr_shows_source_and_format(self, simple_lp_file: Path) -> None:
        assert repr(LpParser(simple_lp_file)) == f"LpParser(lp_file='{simple_lp_file}', format='lp')"
        from_string = LpParser.from_string("Minimize\n obj: x\nSubject To\n c1: x >= 1\nEnd\n")
        assert repr(from_string) == "LpParser(lp_file='<string>', format='lp')"

    def test_create_parser_nonexistent_file(self) -> None:
        with pytest.raises(FileNotFoundError) as info:
            LpParser("nonexistent.lp")
        assert info.value.errno == errno.ENOENT
        assert info.value.filename == "nonexistent.lp"

    def test_create_parser_from_directory_raises_is_a_directory(self, tmp_path: Path) -> None:
        with pytest.raises(IsADirectoryError) as info:
            LpParser.from_file(tmp_path)
        assert info.value.filename == str(tmp_path)

    @pytest.mark.parametrize(
        ("fixture_name", "expected_sense", "expected_vars", "expected_constraints"),
        [
            ("simple_lp_file", "maximize", 2, 2),
            ("minimize_lp_file", "minimize", 3, 2),
        ],
    )
    def test_parse_file(
        self,
        fixture_name: str,
        expected_sense: str,
        expected_vars: int,
        expected_constraints: int,
        request: pytest.FixtureRequest,
    ) -> None:
        lp_file: Path = request.getfixturevalue(fixture_name)
        parser = LpParser(str(lp_file))
        parser.parse()

        assert parser.sense == expected_sense
        assert len(parser.variables) == expected_vars
        assert len(parser.constraints) == expected_constraints
        assert len(parser.objectives) == 1


class TestLpParserComponents:
    def test_get_objectives(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        objectives = parser.objectives
        assert len(objectives) == 1

        obj = objectives[0]
        assert obj["name"] == "OBJ"
        assert len(obj["coefficients"]) == 2

        coeffs = {c["name"]: c["value"] for c in obj["coefficients"]}
        assert coeffs["x1"] == 1.0
        assert coeffs["x2"] == 2.0

    def test_get_constraints(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        constraints = parser.constraints
        assert len(constraints) == 2

        constraint_names = {c["name"] for c in constraints}
        assert "C1" in constraint_names
        assert "C2" in constraint_names

        # Find C1 constraint
        c1 = next(c for c in constraints if c["name"] == "C1")
        assert c1["type"] == "standard"
        assert c1["rhs"] == 3.0
        assert "LTE" in c1["operator"]

        coeffs = {c["name"]: c["value"] for c in c1["coefficients"]}
        assert coeffs["x1"] == 1.0
        assert coeffs["x2"] == 1.0

    def test_get_variables(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        variables = parser.variables
        assert len(variables) == 2
        assert "x1" in variables
        assert "x2" in variables

        x1 = variables["x1"]
        assert x1["name"] == "x1"
        assert x1["kind"] == "Continuous"
        assert x1["lower"] is not None

    def test_targeted_accessors_match_snapshots(self, afiro_lp_file: Path) -> None:
        parser = LpParser(afiro_lp_file)
        constraints = parser.constraints
        variables = parser.variables
        assert parser.num_constraints == len(constraints)
        assert parser.num_variables == len(variables)
        assert parser.num_objectives == len(parser.objectives)
        for constraint in constraints:
            assert parser.get_constraint(constraint["name"]) == constraint
        for name, variable in variables.items():
            assert parser.get_variable(name) == variable

    def test_targeted_accessors_cover_every_constraint_type(self) -> None:
        lp = (
            "Minimize\n obj: x + y + r\nSubject To\n c1: x + y >= 1\n q: y + [ y ^ 2 ] <= 4\n"
            " ind: b = 1 -> x + y <= 3\nGeneral Constraints\n g: r = MIN ( x , y , 2 )\n"
            "Binaries\n b\nSOS\n s1: S1:: x:1 y:2\nEnd\n"
        )
        parser = LpParser.from_string(lp)
        assert {c["type"] for c in parser.constraints} == {"standard", "quadratic", "indicator", "general", "sos"}
        for constraint in parser.constraints:
            assert parser.get_constraint(constraint["name"]) == constraint

    def test_targeted_accessors_reject_missing_and_empty_names(self, simple_lp_file: Path) -> None:
        parser = LpParser(simple_lp_file)
        with pytest.raises(LpObjectNotFoundError):
            parser.get_constraint("missing")
        with pytest.raises(LpObjectNotFoundError):
            parser.get_variable("missing")
        # A variable name is not a constraint, and vice versa.
        with pytest.raises(LpObjectNotFoundError):
            parser.get_constraint("x1")
        with pytest.raises(LpObjectNotFoundError):
            parser.get_variable("C1")
        with pytest.raises(LpInvalidValueError):
            parser.get_constraint("")
        with pytest.raises(LpInvalidValueError):
            parser.get_variable("")

    def test_undeclared_bounds_are_none_and_free_is_infinite(self) -> None:
        """None means "not declared" (format default applies), not "unbounded"."""
        parser = LpParser.from_string("Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\nBounds\n y free\nEnd\n")
        variables = parser.variables
        assert (variables["x"]["lower"], variables["x"]["upper"]) == (None, None)
        assert (variables["y"]["lower"], variables["y"]["upper"]) == (float("-inf"), float("inf"))


class TestLpParserCSV:
    def test_to_csv_creates_files(self, simple_lp_file: Path, tmp_path: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        parser.to_csv(str(tmp_path))
        expected_files = ["constraints.csv", "objectives.csv", "variables.csv"]
        for filename in expected_files:
            file_path = tmp_path / filename
            assert file_path.exists(), f"{filename} should be created"
            assert file_path.stat().st_size > 0, f"{filename} should not be empty"

    def test_to_csv_invalid_directory(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()
        with pytest.raises(NotADirectoryError):
            parser.to_csv("/nonexistent/directory")

    def test_to_csv_auto_parse(self, simple_lp_file: Path, tmp_path: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.to_csv(str(tmp_path))
        constraints_file = tmp_path / "constraints.csv"
        assert constraints_file.exists()


class TestLpParserProperties:
    @pytest.mark.parametrize(
        ("fixture_name", "expected_name"),
        [
            ("afiro_lp_file", "afiro.mps"),
            ("simple_lp_file", None),
        ],
    )
    def test_get_name(
        self,
        fixture_name: str,
        expected_name: str | None,
        request: pytest.FixtureRequest,
    ) -> None:
        lp_file: Path = request.getfixturevalue(fixture_name)
        parser = LpParser(str(lp_file))
        parser.parse()
        assert parser.name == expected_name

    @pytest.mark.parametrize(
        ("fixture_name", "expected_sense"),
        [
            ("simple_lp_file", "maximize"),
            ("minimize_lp_file", "minimize"),
        ],
    )
    def test_get_sense(
        self,
        fixture_name: str,
        expected_sense: str,
        request: pytest.FixtureRequest,
    ) -> None:
        lp_file: Path = request.getfixturevalue(fixture_name)
        parser = LpParser(str(lp_file))
        parser.parse()
        assert parser.sense == expected_sense

    def test_problem_data_available_without_explicit_parse(
        self,
        temp_lp_file: Callable[[str], AbstractContextManager[str]],
    ) -> None:
        """Construction parses, so the accessors work without calling parse()."""
        content = """Maximize
OBJ: x1
Subject To
c1: x1 <= 4
End"""
        with temp_lp_file(content) as filepath:
            parser = LpParser(filepath)
            assert set(parser.variables) == {"x1"}
            assert [constraint["name"] for constraint in parser.constraints] == ["c1"]
            assert [objective["name"] for objective in parser.objectives] == ["OBJ"]


class TestAllResourceFiles:
    """Parse every .lp file in rust/resources/ — mirrors Rust test_from_file.rs coverage."""

    @pytest.mark.parametrize(
        "lp_file",
        [pytest.param(f, id=f.stem) for f in collect_lp_resource_files() if f.name not in EXPECTED_PARSE_FAILURES],
    )
    def test_parse_succeeds(self, lp_file: Path) -> None:
        parser = LpParser(str(lp_file))
        parser.parse()
        assert len(parser.objectives) >= 1
        assert len(parser.variables) >= 1

    @pytest.mark.parametrize(
        "lp_file",
        [pytest.param(f, id=f.stem) for f in collect_lp_resource_files() if f.name in EXPECTED_PARSE_FAILURES],
    )
    def test_parse_fails(self, lp_file: Path) -> None:
        # Construction parses, so the failure surfaces there.
        with pytest.raises(RuntimeError):
            LpParser(str(lp_file))


class TestMutationErrors:
    """The exception raised must reflect the *kind* of failure, not the method
    that failed. Both types subclass RuntimeError, so `except RuntimeError`
    keeps working."""

    SOS_LP = "Minimize\n obj: V1 + V2\nSubject To\n c1: V1 + V2 >= 1\nSOS\ncsos1: S1:: V1:1 V2:2\nEnd\n"

    @pytest.fixture
    def parser(self) -> LpParser:
        return LpParser.from_string(self.SOS_LP, "lp")

    def test_missing_name_raises_not_found(self, parser: LpParser) -> None:
        with pytest.raises(LpObjectNotFoundError):
            parser.update_constraint_rhs("does_not_exist", 1.0)

    def test_unsupported_operation_raises_invalid_value(self, parser: LpParser) -> None:
        # An SOS constraint has no RHS: that is an invalid operation, not a
        # missing object.
        with pytest.raises(LpInvalidValueError):
            parser.update_constraint_rhs("csos1", 1.0)

    def test_non_finite_value_raises_invalid_value(self, parser: LpParser) -> None:
        for bad in (float("nan"), float("inf"), float("-inf")):
            with pytest.raises(LpInvalidValueError):
                parser.update_constraint_rhs("c1", bad)

    def test_empty_name_raises_invalid_value(self, parser: LpParser) -> None:
        with pytest.raises(LpInvalidValueError):
            parser.rename_variable("V1", "")

    @pytest.mark.parametrize(
        ("method", "args"),
        [
            ("remove_objective", ("",)),
            ("remove_constraint", ("",)),
            ("remove_variable", ("",)),
            ("update_variable_type", ("", "binary")),
            ("update_variable_type", ("", "continuous")),
        ],
    )
    def test_empty_name_on_remove_or_retype_raises_invalid_value(
        self, parser: LpParser, method: str, args: tuple[str, ...]
    ) -> None:
        with pytest.raises(LpInvalidValueError, match="must not be empty"):
            getattr(parser, method)(*args)

    def test_rejected_mutation_leaves_the_model_writable(self, parser: LpParser) -> None:
        with pytest.raises(LpInvalidValueError):
            parser.update_constraint_rhs("c1", float("nan"))
        assert "NaN" not in parser.to_lp_string()


class TestReparse:
    """parse() must re-read the source with the parser it was built with."""

    def test_reparse_mps_file(self, simple_lp_file: Path, tmp_path: Path) -> None:
        mps_path = tmp_path / "simple.mps"
        LpParser(str(simple_lp_file)).save_to_mps(str(mps_path))
        parser = LpParser(str(mps_path))
        parser.parse()
        assert len(parser.variables) == 2

    def test_reparse_explicit_format(self, simple_lp_file: Path, tmp_path: Path) -> None:
        mps_path = tmp_path / "simple.txt"
        LpParser(str(simple_lp_file)).save_to_mps(str(mps_path))
        parser = LpParser.from_file(str(mps_path), "mps")
        parser.parse()
        assert len(parser.variables) == 2

    def test_reparse_string_backed_parser_raises(self) -> None:
        parser = LpParser.from_string("Minimize\n obj: x\nSubject To\n c1: x >= 1\nEnd\n")
        with pytest.raises(LpInvalidValueError, match="built from a string"):
            parser.parse()


class TestPathLikeArguments:
    """Every path argument accepts os.PathLike as well as str."""

    def test_pathlib_paths_accepted(self, simple_lp_file: Path, tmp_path: Path) -> None:
        parser = LpParser(simple_lp_file)
        assert parser.lp_file == str(simple_lp_file)
        assert len(LpParser.from_file(simple_lp_file).variables) == 2

        parser.save_to_file(tmp_path / "out.lp")
        parser.save_to_mps(tmp_path / "out.mps")
        parser.to_csv(tmp_path)
        assert (tmp_path / "out.lp").is_file()
        assert (tmp_path / "out.mps").is_file()
        assert (tmp_path / "variables.csv").is_file()


class TestIoErrors:
    """I/O failures raise OSError subclasses, not LpParseError."""

    def test_reparse_deleted_file_raises_file_not_found(self, simple_lp_file: Path, tmp_path: Path) -> None:
        path = tmp_path / "gone.lp"
        path.write_text(simple_lp_file.read_text())
        parser = LpParser(path)
        path.unlink()
        with pytest.raises(FileNotFoundError) as info:
            parser.parse()
        assert not isinstance(info.value, RuntimeError)
        assert info.value.filename == str(path)

    def test_non_utf8_file_raises_parse_error_with_encoding_hint(self, tmp_path: Path) -> None:
        path = tmp_path / "latin1.lp"
        path.write_bytes("Minimize\n obj: x\nSubject To\n c\u00e9: x >= 1\nEnd\n".encode("latin-1"))
        with pytest.raises(LpParseError, match="not valid UTF-8"):
            LpParser(path)

    def test_save_into_missing_directory_raises_os_error(self, simple_lp_file: Path, tmp_path: Path) -> None:
        parser = LpParser(simple_lp_file)
        with pytest.raises(FileNotFoundError):
            parser.save_to_file(tmp_path / "missing" / "out.lp")
        with pytest.raises(FileNotFoundError):
            parser.save_to_mps(tmp_path / "missing" / "out.mps")

    def test_save_onto_directory_raises_os_error(self, simple_lp_file: Path, tmp_path: Path) -> None:
        with pytest.raises(IsADirectoryError):
            LpParser(simple_lp_file).save_to_file(tmp_path)


class TestUpdateVariableType:
    LP = "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\nBounds\n 2 <= x <= 5\nGenerals\n x\nEnd\n"

    def test_continuous_changes_kind_and_keeps_bounds(self) -> None:
        parser = LpParser.from_string(self.LP)
        parser.update_variable_type("x", "continuous")
        x = parser.variables["x"]
        assert (x["kind"], x["lower"], x["upper"]) == ("Continuous", 2.0, 5.0)

    def test_free_sets_continuous_with_infinite_bounds(self) -> None:
        parser = LpParser.from_string(self.LP)
        parser.update_variable_type("x", "free")
        x = parser.variables["x"]
        assert (x["kind"], x["lower"], x["upper"]) == ("Continuous", float("-inf"), float("inf"))

    def test_generals_and_semi_continuous_make_a_semi_integer(self) -> None:
        parser = LpParser.from_string(self.LP.replace("End", "Semi-Continuous\n x\nEnd"))
        x = parser.variables["x"]
        assert (x["kind"], x["lower"], x["upper"]) == ("SemiInteger", 2.0, 5.0)
        assert "Semi-Continuous\n x" in parser.to_lp_string()

    def test_semiinteger_sets_the_kind(self) -> None:
        parser = LpParser.from_string(self.LP)
        parser.update_variable_type("y", "semiinteger")
        assert parser.variables["y"]["kind"] == "SemiInteger"

    def test_continuous_on_missing_variable_raises_not_found(self) -> None:
        parser = LpParser.from_string(self.LP)
        with pytest.raises(LpObjectNotFoundError):
            parser.update_variable_type("missing", "continuous")


class TestConstraintClass:
    LP = (
        "Minimize\n obj: x + y\nSubject To\n c1: x + y >= 1\n"
        "Lazy Constraints\n l1: x <= 4\nUser Cuts\n u1: x + y <= 9\nEnd\n"
    )

    def test_class_of_each_constraint(self) -> None:
        parser = LpParser.from_string(self.LP)
        classes = {c["name"]: c["class"] for c in parser.constraints if c["type"] == "standard"}
        assert classes == {"c1": "normal", "l1": "lazy", "u1": "user_cut"}

    def test_sections_round_trip(self) -> None:
        written = LpParser.from_string(self.LP).to_lp_string()
        assert "Lazy Constraints\n l1: x <= 4" in written
        assert "User Cuts\n u1: x + y <= 9" in written


class TestIndicatorConstraint:
    LP = "Minimize\n obj: x + b\nSubject To\n ind: b = 0 -> x >= 2\nBinaries\n b\nEnd\n"

    def test_indicator_is_exposed(self) -> None:
        (constraint,) = LpParser.from_string(self.LP).constraints
        assert constraint["type"] == "indicator"
        assert (constraint["indicator_variable"], constraint["indicator_value"]) == ("b", 0)
        assert (constraint["operator"], constraint["rhs"], constraint["class"]) == ("GTE", 2.0, "normal")

    def test_indicator_round_trips(self) -> None:
        assert " ind: b = 0 -> x >= 2" in LpParser.from_string(self.LP).to_lp_string()


class TestQuadratic:
    LP = "Minimize\n obj: x + [ x ^ 2 + 4 x * y ] / 2\nSubject To\n q: y + [ y ^ 2 ] <= 4\nEnd\n"

    def test_objective_terms_are_halved(self) -> None:
        (objective,) = LpParser.from_string(self.LP).objectives
        assert objective["quadratic"] == [
            {"var1": "x", "var2": "x", "coefficient": 0.5},
            {"var1": "x", "var2": "y", "coefficient": 2.0},
        ]

    def test_quadratic_constraint(self) -> None:
        (constraint,) = LpParser.from_string(self.LP).constraints
        assert constraint["type"] == "quadratic"
        assert constraint["quadratic"] == [{"var1": "y", "var2": "y", "coefficient": 1.0}]

    def test_round_trip(self) -> None:
        written = LpParser.from_string(self.LP).to_lp_string()
        assert " obj: x + [ x ^ 2 + 4 x * y ] / 2" in written
        assert " q: y + [ y ^ 2 ] <= 4" in written


class TestGeneralConstraint:
    LP = "Maximize\n obj: r\nSubject To\n c1: x + y <= 4\nGeneral Constraints\n g: r = MIN ( x , y , 2 )\nEnd\n"

    def test_general_constraint_is_exposed(self) -> None:
        general = next(c for c in LpParser.from_string(self.LP).constraints if c["type"] == "general")
        assert (general["resultant"], general["function"], general["arguments"], general["constant"]) == (
            "r",
            "MIN",
            ["x", "y"],
            2.0,
        )

    def test_round_trip(self) -> None:
        assert "General Constraints\n g: r = MIN ( x , y , 2 )" in LpParser.from_string(self.LP).to_lp_string()

    def test_mps_writer_rejection_raises_invalid_value(self, tmp_path: Path) -> None:
        # MPS has no general-constraint section: the writer's validation error
        # must surface as LpInvalidValueError, not a bare RuntimeError.
        parser = LpParser.from_string(self.LP)
        with pytest.raises(LpInvalidValueError, match="cannot be written to MPS"):
            parser.to_mps_string()
        with pytest.raises(LpInvalidValueError, match="cannot be written to MPS"):
            parser.save_to_mps(tmp_path / "out.mps")


class TestMultiObjective:
    LP = (
        "Minimize multi-objectives\n Cost: Priority=2 Weight=1 AbsTol=0 RelTol=0.1\n  x + y\n"
        " Time:\n  x\nSubject To\n c1: x + y >= 1\nEnd\n"
    )

    def test_attributes_are_exposed(self) -> None:
        cost, time = LpParser.from_string(self.LP).objectives
        assert cost["attributes"] == {"priority": 2, "weight": 1.0, "abs_tol": 0.0, "rel_tol": 0.1}
        assert time["attributes"] == {"priority": None, "weight": None, "abs_tol": None, "rel_tol": None}

    def test_round_trip(self) -> None:
        written = LpParser.from_string(self.LP).to_lp_string()
        assert written.startswith("Minimize multi-objectives\n Cost: Priority=2 Weight=1 AbsTol=0 RelTol=0.1\n")


class TestThresholdValidation:
    LP = "Minimize\n obj: x\nSubject To\n c1: x >= 1000\nEnd\n"

    @staticmethod
    def _messages(parser: LpParser, **thresholds: float) -> list[str]:
        return [issue["message"] for issue in parser.analyze(**thresholds)["issues"]]

    def test_large_rhs_threshold_is_independent_of_coefficient_threshold(self) -> None:
        parser = LpParser.from_string(self.LP)
        assert not any("Large RHS" in m for m in self._messages(parser, large_coeff_threshold=10.0))
        assert any("Large RHS" in m for m in self._messages(parser, large_rhs_threshold=10.0))

    @pytest.mark.parametrize(
        "name", ["large_coeff_threshold", "small_coeff_threshold", "ratio_threshold", "large_rhs_threshold"]
    )
    @pytest.mark.parametrize("bad", [0.0, -1.0, float("nan"), float("inf")])
    def test_invalid_threshold_raises(self, name: str, bad: float) -> None:
        parser = LpParser.from_string(self.LP)
        with pytest.raises(LpInvalidValueError, match=name):
            parser.analyze(**{name: bad})

    def test_zero_max_line_length_raises(self) -> None:
        with pytest.raises(LpInvalidValueError, match="max_line_length"):
            LpParser.from_string(self.LP).to_lp_string(max_line_length=0)
