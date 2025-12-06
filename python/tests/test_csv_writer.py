from pathlib import Path

from parse_lp import LpParser


def test_writing_to_file(afiro_lp_file: Path, tmp_path: Path) -> None:
    parser = LpParser(str(afiro_lp_file))
    parser.to_csv(str(tmp_path))
    assert len(list(tmp_path.iterdir())) == 3
