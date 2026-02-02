import tempfile
from collections.abc import Generator
from contextlib import AbstractContextManager, contextmanager
from pathlib import Path
from typing import Callable

import pytest


@pytest.fixture
def simple_lp_file() -> Path:
    return Path(__file__).parent / "fixtures" / "simple.lp"


@pytest.fixture
def minimize_lp_file() -> Path:
    return Path(__file__).parent / "fixtures" / "minimize.lp"


@pytest.fixture
def afiro_lp_file() -> Path:
    return Path(__file__).parent / "fixtures" / "afiro.lp"


@pytest.fixture
def temp_lp_file() -> Callable[[str], AbstractContextManager[str]]:
    @contextmanager
    def _create_file(content: str) -> Generator[str, None, None]:
        with tempfile.NamedTemporaryFile(mode="w", suffix=".lp", delete=False) as f:
            f.write(content)
            temp_path = f.name
        try:
            yield temp_path
        finally:
            Path(temp_path).unlink(missing_ok=True)

    return _create_file
