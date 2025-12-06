from pathlib import Path

import pytest

from parse_lp import LpParser


class TestLpWriter:
    def test_to_lp_string_basic(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        lp_string: str = parser.to_lp_string()

        assert "Maximize" in lp_string
        assert "OBJ:" in lp_string
        assert "x1" in lp_string
        assert "x2" in lp_string
        assert "Subject To" in lp_string
        assert "C1:" in lp_string
        assert "C2:" in lp_string
        assert "End" in lp_string

    def test_to_lp_string_with_options(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        lp_string = parser.to_lp_string_with_options(
            include_problem_name=False,
            max_line_length=80,
            decimal_precision=2,
            include_section_spacing=False,
        )

        assert "Maximize" in lp_string
        assert "\\Problem name:" not in lp_string
        assert "End" in lp_string

    def test_save_to_file(self, simple_lp_file: Path, tmp_path: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        output_path = tmp_path / "output.lp"
        parser.save_to_file(str(output_path))

        assert output_path.exists()
        content = output_path.read_text()
        assert "Maximize" in content
        assert "End" in content

    def test_round_trip_parsing(self, simple_lp_file: Path, tmp_path: Path) -> None:
        parser1 = LpParser(str(simple_lp_file))
        parser1.parse()

        lp_string = parser1.to_lp_string()

        temp_path = tmp_path / "round_trip.lp"
        temp_path.write_text(lp_string)

        parser2 = LpParser(str(temp_path))
        parser2.parse()

        assert parser1.sense == parser2.sense
        assert parser1.variable_count() == parser2.variable_count()
        assert parser1.constraint_count() == parser2.constraint_count()
        assert parser1.objective_count() == parser2.objective_count()


class TestLpModification:
    def test_update_objective_coefficient(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        parser.update_objective_coefficient("OBJ", "x1", 5.0)

        objectives = parser.objectives
        obj = objectives[0]
        coeffs = {c["name"]: c["value"] for c in obj["coefficients"]}
        assert coeffs["x1"] == 5.0

    def test_update_objective_coefficient_new_variable(
        self, simple_lp_file: Path
    ) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        original_var_count = parser.variable_count()

        parser.update_objective_coefficient("OBJ", "x3", 3.0)

        assert parser.variable_count() == original_var_count + 1
        objectives = parser.objectives
        obj = objectives[0]
        coeffs = {c["name"]: c["value"] for c in obj["coefficients"]}
        assert "x3" in coeffs
        assert coeffs["x3"] == 3.0

    def test_rename_objective(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        parser.rename_objective("OBJ", "PROFIT")

        objectives = parser.objectives
        obj = objectives[0]
        assert obj["name"] == "PROFIT"

    def test_update_constraint_coefficient(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        parser.update_constraint_coefficient("C1", "x1", 2.0)

        constraints = parser.constraints
        c1 = next(c for c in constraints if c["name"] == "C1")
        coeffs = {c["name"]: c["value"] for c in c1["coefficients"]}
        assert coeffs["x1"] == 2.0

    def test_update_constraint_rhs(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        parser.update_constraint_rhs("C1", 5.0)

        constraints = parser.constraints
        c1 = next(c for c in constraints if c["name"] == "C1")
        assert c1["rhs"] == 5.0

    def test_rename_constraint(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        parser.rename_constraint("C1", "CAPACITY")

        constraints = parser.constraints
        constraint_names = {c["name"] for c in constraints}
        assert "CAPACITY" in constraint_names
        assert "C1" not in constraint_names

    def test_rename_variable(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        parser.rename_variable("x1", "production")

        objectives = parser.objectives
        obj = objectives[0]
        coeff_names = {c["name"] for c in obj["coefficients"]}
        assert "production" in coeff_names
        assert "x1" not in coeff_names

        constraints = parser.constraints
        for constraint in constraints:
            if constraint["type"] == "standard":
                coeff_names = {c["name"] for c in constraint["coefficients"]}
                if "production" in coeff_names:
                    assert "x1" not in coeff_names

    def test_update_variable_type(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        parser.update_variable_type("x1", "integer")

        variables = parser.variables
        assert "Integer" in variables["x1"]["var_type"]

    def test_update_variable_type_invalid(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        with pytest.raises(RuntimeError, match="Unknown variable type"):
            parser.update_variable_type("x1", "invalid_type")

    def test_set_problem_name(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        parser.set_problem_name("My Optimization Problem")

        assert parser.name == "My Optimization Problem"

    def test_set_sense(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        original_sense = parser.sense
        assert original_sense == "maximize"

        parser.set_sense("minimize")
        assert parser.sense == "minimize"

    def test_set_sense_invalid(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        with pytest.raises(RuntimeError, match="Invalid sense"):
            parser.set_sense("invalid_sense")

    def test_remove_constraint(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        original_count = parser.constraint_count()
        parser.remove_constraint("C1")

        assert parser.constraint_count() == original_count - 1
        constraints = parser.constraints
        constraint_names = {c["name"] for c in constraints}
        assert "C1" not in constraint_names

    def test_remove_variable(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        original_count = parser.variable_count()
        parser.remove_variable("x2")

        assert parser.variable_count() == original_count - 1
        variables = parser.variables
        assert "x2" not in variables

    def test_complex_modification_workflow(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        parser.update_objective_coefficient("OBJ", "x1", 5.0)
        parser.update_objective_coefficient("OBJ", "x3", 1.5)
        parser.rename_objective("OBJ", "PROFIT")
        parser.update_constraint_coefficient("C1", "x3", 0.5)
        parser.update_constraint_rhs("C1", 10.0)
        parser.rename_constraint("C1", "CAPACITY")
        parser.rename_variable("x2", "production")
        parser.update_variable_type("x1", "integer")
        parser.set_problem_name("Modified Problem")

        assert parser.name == "Modified Problem"

        objectives = parser.objectives
        obj = objectives[0]
        assert obj["name"] == "PROFIT"
        coeffs = {c["name"]: c["value"] for c in obj["coefficients"]}
        assert coeffs["x1"] == 5.0
        assert coeffs["x3"] == 1.5
        assert "production" in coeffs

        constraints = parser.constraints
        capacity = next(c for c in constraints if c["name"] == "CAPACITY")
        assert capacity["rhs"] == 10.0
        coeffs = {c["name"]: c["value"] for c in capacity["coefficients"]}
        assert coeffs["x3"] == 0.5

        variables = parser.variables
        assert "Integer" in variables["x1"]["var_type"]
        assert "production" in variables
        assert "x2" not in variables

        lp_string = parser.to_lp_string()
        assert "Modified Problem" in lp_string
        assert "PROFIT:" in lp_string
        assert "CAPACITY:" in lp_string
        assert "production" in lp_string


class TestLpModificationErrors:
    def test_update_nonexistent_objective(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        with pytest.raises(RuntimeError, match="not found"):
            parser.update_objective_coefficient("NONEXISTENT", "x1", 5.0)

    def test_update_nonexistent_constraint(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        with pytest.raises(RuntimeError, match="not found"):
            parser.update_constraint_coefficient("NONEXISTENT", "x1", 5.0)

    def test_rename_nonexistent_variable(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        with pytest.raises(RuntimeError, match="not found"):
            parser.rename_variable("nonexistent", "new_name")

    def test_modification_without_parse(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))

        with pytest.raises(RuntimeError, match="Must call parse"):
            parser.update_objective_coefficient("OBJ", "x1", 5.0)
