import json
from pathlib import Path

import pytest

from parse_lp.cli import main


class TestDiffExitCodes:
    def test_identical_files_exit_0(self, simple_lp_file: Path) -> None:
        path = str(simple_lp_file)
        assert main(["diff", path, path]) == 0

    def test_different_files_exit_1(self, simple_lp_file: Path, minimize_lp_file: Path) -> None:
        assert main(["diff", str(simple_lp_file), str(minimize_lp_file)]) == 1

    def test_nonexistent_file_exit_2(self, simple_lp_file: Path) -> None:
        assert main(["diff", str(simple_lp_file), "/nonexistent/file.lp"]) == 2

    def test_no_subcommand_exit_2(self) -> None:
        assert main([]) == 2


class TestDiffJson:
    def test_json_output_is_valid(
        self, simple_lp_file: Path, minimize_lp_file: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        main(["diff", "--json", str(simple_lp_file), str(minimize_lp_file)])
        captured = capsys.readouterr()
        data = json.loads(captured.out)
        assert "identical" in data
        assert data["identical"] is False
        assert "added_variables" in data
        assert "removed_variables" in data
        assert "sense_changed" in data

    def test_json_identical(self, simple_lp_file: Path, capsys: pytest.CaptureFixture[str]) -> None:
        path = str(simple_lp_file)
        main(["diff", "--json", path, path])
        captured = capsys.readouterr()
        data = json.loads(captured.out)
        assert data["identical"] is True


class TestDiffQuiet:
    def test_quiet_no_stdout(
        self, simple_lp_file: Path, minimize_lp_file: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        main(["diff", "-q", str(simple_lp_file), str(minimize_lp_file)])
        captured = capsys.readouterr()
        assert captured.out == ""

    def test_quiet_exit_code(self, simple_lp_file: Path, minimize_lp_file: Path) -> None:
        assert main(["diff", "-q", str(simple_lp_file), str(minimize_lp_file)]) == 1

    def test_quiet_identical_exit_code(self, simple_lp_file: Path) -> None:
        path = str(simple_lp_file)
        assert main(["diff", "-q", path, path]) == 0


class TestDiffSummary:
    def test_summary_omits_coefficient_details(
        self, simple_lp_file: Path, minimize_lp_file: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        main(["diff", "--summary", str(simple_lp_file), str(minimize_lp_file)])
        captured = capsys.readouterr()
        assert "Summary" in captured.out
        # The detail sections should not appear (the word "Objectives" may appear in the summary counts)
        assert "Objectives\n----------" not in captured.out
        assert "Constraint Details\n------------------" not in captured.out


class TestDiffHumanOutput:
    def test_human_output_markers(
        self, simple_lp_file: Path, minimize_lp_file: Path, capsys: pytest.CaptureFixture[str]
    ) -> None:
        main(["diff", str(simple_lp_file), str(minimize_lp_file)])
        captured = capsys.readouterr()
        assert "---" in captured.out
        assert "+++" in captured.out
        assert "Summary" in captured.out

    def test_identical_message(self, simple_lp_file: Path, capsys: pytest.CaptureFixture[str]) -> None:
        path = str(simple_lp_file)
        main(["diff", path, path])
        captured = capsys.readouterr()
        assert "identical" in captured.out
