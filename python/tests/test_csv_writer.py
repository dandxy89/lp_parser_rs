import os
import tempfile

from parse_lp import LpParser


def test_writing_to_file() -> None:
    lp_file = os.path.join(os.path.dirname(__file__), "../resources/afiro.lp")
    with tempfile.TemporaryDirectory() as td:
        parser = LpParser(lp_file)
        parser.to_csv(td)
        assert len(os.listdir(td)) == 3
