import os
import tempfile
from pathlib import Path

import pytest

from parse_lp import LpParser


@pytest.fixture
def temp_lp_file():
    def _create_file(content):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".lp", delete=False) as f:
            f.write(content)
            yield f.name
        os.unlink(f.name)

    return _create_file


@pytest.fixture
def simple_lp_file():
    return Path(__file__).parent / "fixtures" / "simple.lp"


@pytest.fixture
def minimize_lp_file():
    """Path to minimize.lp fixture file"""
    return Path(__file__).parent / "fixtures" / "minimize.lp"


@pytest.fixture
def afiro_lp_file():
    """Path to minimize.lp fixture file"""
    return Path(__file__).parent / "fixtures" / "afiro.lp"


class TestLpParserBasic:
    def test_create_parser(self, simple_lp_file):
        parser = LpParser(str(simple_lp_file))
        assert parser.lp_file == str(simple_lp_file)

    def test_create_parser_nonexistent_file(self):
        with pytest.raises(FileExistsError):
            LpParser("nonexistent.lp")

    def test_parse_simple_file(self, simple_lp_file):
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        assert parser.sense == "maximize"
        assert parser.variable_count() == 2
        assert parser.constraint_count() == 2
        assert parser.objective_count() == 1

    def test_parse_minimize_file(self, minimize_lp_file):
        parser = LpParser(str(minimize_lp_file))
        parser.parse()

        assert parser.sense == "minimize"
        assert parser.variable_count() == 3
        assert parser.constraint_count() == 2
        assert parser.objective_count() == 1


class TestLpParserComponents:
    def test_get_objectives(self, simple_lp_file):
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

    def test_get_constraints(self, simple_lp_file):
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

    def test_get_variables(self, simple_lp_file):
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        variables = parser.variables
        assert len(variables) == 2
        assert "x1" in variables
        assert "x2" in variables

        x1 = variables["x1"]
        assert x1["name"] == "x1"
        assert "LowerBound" in x1["var_type"]


class TestLpParserDiff:
    def test_compare_same_parser(self, simple_lp_file):
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        diff = parser.compare(parser)

        assert not diff["name_changed"]
        assert not diff["sense_changed"]
        assert diff["variable_count_diff"] == 0
        assert diff["constraint_count_diff"] == 0
        assert diff["objective_count_diff"] == 0
        assert len(diff["added_variables"]) == 0
        assert len(diff["removed_variables"]) == 0
        assert len(diff["modified_variables"]) == 0

    def test_compare_different_parsers(self, simple_lp_file, minimize_lp_file):
        parser1 = LpParser(str(simple_lp_file))
        parser1.parse()
        parser2 = LpParser(str(minimize_lp_file))
        parser2.parse()

        diff = parser1.compare(parser2)

        assert diff["sense_changed"]
        assert diff["variable_count_diff"] == -1  # p1 has 2, p2 has 3
        assert diff["constraint_count_diff"] == 0  # both have 2
        assert "x3" in diff["added_variables"]
        assert len(diff["removed_variables"]) == 0

    def test_compare_unparsed_parser(self, simple_lp_file, temp_lp_file):
        parser1 = LpParser(str(simple_lp_file))
        parser1.parse()

        content = """Maximize
OBJ: x1
End"""
        for filepath in temp_lp_file(content):
            parser2 = LpParser(filepath)
            with pytest.raises(RuntimeError, match="Must call parse\\(\\) first"):
                parser1.compare(parser2)


class TestLpParserCSV:
    def test_to_csv_creates_files(self, simple_lp_file):
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        with tempfile.TemporaryDirectory() as tmpdir:
            parser.to_csv(tmpdir)
            expected_files = ["constraints.csv", "objectives.csv", "variables.csv"]
            for filename in expected_files:
                file_path = Path(tmpdir) / filename
                assert file_path.exists(), f"{filename} should be created"
                assert file_path.stat().st_size > 0, f"{filename} should not be empty"

    def test_to_csv_invalid_directory(self, simple_lp_file):
        parser = LpParser(str(simple_lp_file))
        parser.parse()
        with pytest.raises(NotADirectoryError):
            parser.to_csv("/nonexistent/directory")

    def test_to_csv_auto_parse(self, simple_lp_file):
        parser = LpParser(str(simple_lp_file))
        with tempfile.TemporaryDirectory() as tmpdir:
            parser.to_csv(tmpdir)
            constraints_file = Path(tmpdir) / "constraints.csv"
            assert constraints_file.exists()


class TestLpParserProperties:
    def test_get_name_when_present(self, afiro_lp_file):
        parser = LpParser(str(afiro_lp_file))
        parser.parse()
        assert parser.name == "Problem name: afiro.mps"

    def test_get_name_when_absent(self, simple_lp_file):
        parser = LpParser(str(simple_lp_file))
        parser.parse()
        assert parser.name is None

    def test_get_sense_maximize(self, simple_lp_file):
        parser = LpParser(str(simple_lp_file))
        parser.parse()
        assert parser.sense == "maximize"

    def test_get_sense_minimize(self, minimize_lp_file):
        parser = LpParser(str(minimize_lp_file))
        parser.parse()
        assert parser.sense == "minimize"

    def test_counts_unparsed(self, temp_lp_file):
        content = """Maximize
OBJ: x1
End"""
        for filepath in temp_lp_file(content):
            parser = LpParser(filepath)
            with pytest.raises(RuntimeError, match="Must call parse\\(\\) first"):
                parser.variable_count()
            with pytest.raises(RuntimeError, match="Must call parse\\(\\) first"):
                parser.constraint_count()
            with pytest.raises(RuntimeError, match="Must call parse\\(\\) first"):
                parser.objective_count()
