from pathlib import Path

import pytest

from parse_lp import LpParser


class TestLpWriter:
    def test_to_lp_string(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        # Basic string generation
        lp_string: str = parser.to_lp_string()
        for expected in [
            "Maximize",
            "OBJ:",
            "x1",
            "x2",
            "Subject To",
            "C1:",
            "C2:",
            "End",
        ]:
            assert expected in lp_string

        # With options
        lp_string = parser.to_lp_string_with_options(
            include_problem_name=False,
            max_line_length=80,
            decimal_precision=2,
            include_section_spacing=False,
        )
        assert "Maximize" in lp_string and "\\Problem name:" not in lp_string

    def test_save_and_round_trip(self, simple_lp_file: Path, tmp_path: Path) -> None:
        parser1 = LpParser(str(simple_lp_file))
        parser1.parse()

        # Save to file
        output_path = tmp_path / "output.lp"
        parser1.save_to_file(str(output_path))
        assert output_path.exists()
        content = output_path.read_text()
        assert "Maximize" in content and "End" in content

        # Round trip
        parser2 = LpParser(str(output_path))
        parser2.parse()
        assert parser1.sense == parser2.sense
        assert parser1.variable_count() == parser2.variable_count()
        assert parser1.constraint_count() == parser2.constraint_count()


class TestLpModification:
    def test_coefficient_updates(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        # Update objective coefficient
        parser.update_objective_coefficient("OBJ", "x1", 5.0)
        coeffs = {c["name"]: c["value"] for c in parser.objectives[0]["coefficients"]}
        assert coeffs["x1"] == 5.0

        # Add new variable via objective
        original_count = parser.variable_count()
        parser.update_objective_coefficient("OBJ", "x3", 3.0)
        assert parser.variable_count() == original_count + 1

        # Update constraint coefficient and RHS
        parser.update_constraint_coefficient("C1", "x1", 2.0)
        parser.update_constraint_rhs("C1", 5.0)
        c1 = next(c for c in parser.constraints if c["name"] == "C1")
        assert c1["rhs"] == 5.0

    def test_rename_operations(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        # Rename objective
        parser.rename_objective("OBJ", "PROFIT")
        assert parser.objectives[0]["name"] == "PROFIT"

        # Rename constraint
        parser.rename_constraint("C1", "CAPACITY")
        constraint_names = {c["name"] for c in parser.constraints}
        assert "CAPACITY" in constraint_names and "C1" not in constraint_names

        # Rename variable - propagates everywhere
        parser.rename_variable("x1", "production")
        coeff_names = {c["name"] for c in parser.objectives[0]["coefficients"]}
        assert "production" in coeff_names and "x1" not in coeff_names

    def test_remove_operations(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        # Remove constraint
        original_count = parser.constraint_count()
        parser.remove_constraint("C1")
        assert parser.constraint_count() == original_count - 1
        assert "C1" not in {c["name"] for c in parser.constraints}

        # Remove variable
        original_count = parser.variable_count()
        parser.remove_variable("x2")
        assert parser.variable_count() == original_count - 1
        assert "x2" not in parser.variables

    def test_variable_type_and_problem_settings(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        # Update variable type
        parser.update_variable_type("x1", "integer")
        assert "Integer" in parser.variables["x1"]["var_type"]

        # Invalid type
        with pytest.raises(RuntimeError, match="Unknown variable type"):
            parser.update_variable_type("x1", "invalid_type")

        # Set problem name
        parser.set_problem_name("Test Problem")
        assert parser.name == "Test Problem"

        # Set sense
        parser.set_sense("minimize")
        assert parser.sense == "minimize"

        # Invalid sense
        with pytest.raises(RuntimeError, match="Invalid sense"):
            parser.set_sense("invalid_sense")

    def test_complex_workflow(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()

        # Apply multiple modifications
        parser.update_objective_coefficient("OBJ", "x1", 5.0)
        parser.update_objective_coefficient("OBJ", "x3", 1.5)
        parser.rename_objective("OBJ", "PROFIT")
        parser.update_constraint_coefficient("C1", "x3", 0.5)
        parser.update_constraint_rhs("C1", 10.0)
        parser.rename_constraint("C1", "CAPACITY")
        parser.rename_variable("x2", "production")
        parser.update_variable_type("x1", "integer")
        parser.set_problem_name("Modified Problem")

        # Verify final state
        assert parser.name == "Modified Problem"
        assert parser.objectives[0]["name"] == "PROFIT"
        coeffs = {c["name"]: c["value"] for c in parser.objectives[0]["coefficients"]}
        assert coeffs["x1"] == 5.0 and coeffs["x3"] == 1.5 and "production" in coeffs

        capacity = next(c for c in parser.constraints if c["name"] == "CAPACITY")
        assert capacity["rhs"] == 10.0

        assert "Integer" in parser.variables["x1"]["var_type"]
        assert "production" in parser.variables and "x2" not in parser.variables


class TestLpModificationErrors:
    @pytest.mark.parametrize(
        ("method", "args"),
        [
            ("update_objective_coefficient", ("NONEXISTENT", "x1", 5.0)),
            ("update_constraint_coefficient", ("NONEXISTENT", "x1", 5.0)),
            ("rename_variable", ("nonexistent", "new_name")),
        ],
    )
    def test_nonexistent_elements(
        self, simple_lp_file: Path, method: str, args: tuple
    ) -> None:
        parser = LpParser(str(simple_lp_file))
        parser.parse()
        with pytest.raises(RuntimeError, match="not found"):
            getattr(parser, method)(*args)

    def test_modification_without_parse(self, simple_lp_file: Path) -> None:
        parser = LpParser(str(simple_lp_file))
        with pytest.raises(RuntimeError, match="Must call parse"):
            parser.update_objective_coefficient("OBJ", "x1", 5.0)
