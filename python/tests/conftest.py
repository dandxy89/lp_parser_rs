import tempfile
from collections.abc import Generator
from contextlib import AbstractContextManager, contextmanager
from pathlib import Path
from typing import Callable

import pytest

FIXTURES_DIR = Path(__file__).parent / "fixtures"
RUST_RESOURCES_DIR = Path(__file__).parent.parent.parent / "rust" / "resources"

# Files that are expected to fail parsing (match Rust test_from_file.rs)
EXPECTED_PARSE_FAILURES = {"invalid.lp", "corrupt.lp"}


def collect_lp_resource_files() -> list[Path]:
    """Collect all .lp files from the shared Rust resources directory."""
    return sorted(RUST_RESOURCES_DIR.glob("*.lp"))


@pytest.fixture
def simple_lp_file() -> Path:
    return FIXTURES_DIR / "simple.lp"


@pytest.fixture
def minimize_lp_file() -> Path:
    return FIXTURES_DIR / "minimize.lp"


@pytest.fixture
def afiro_lp_file() -> Path:
    return RUST_RESOURCES_DIR / "afiro.lp"


@pytest.fixture
def gurobi_implicit_sign_lp_file() -> Path:
    return RUST_RESOURCES_DIR / "gurobi_implicit_sign.lp"


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
