import os
import stat
from pathlib import Path

import pytest

from parse_lp import LpParser


def test_writing_to_file(afiro_lp_file: Path, tmp_path: Path) -> None:
    parser = LpParser(str(afiro_lp_file))
    parser.to_csv(str(tmp_path))
    assert len(list(tmp_path.iterdir())) == 3


@pytest.mark.skipif(not hasattr(os, "geteuid") or os.geteuid() == 0, reason="needs a non-root POSIX user")
def test_permission_denied_raises_permission_error(afiro_lp_file: Path, tmp_path: Path) -> None:
    read_only = tmp_path / "read_only"
    read_only.mkdir()
    read_only.chmod(stat.S_IRUSR | stat.S_IXUSR)
    try:
        with pytest.raises(PermissionError) as info:
            LpParser(afiro_lp_file).to_csv(read_only)
        assert not isinstance(info.value, RuntimeError)
        assert info.value.filename == str(read_only)
    finally:
        read_only.chmod(stat.S_IRWXU)
