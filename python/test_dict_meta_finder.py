import importlib
import shutil
import sys
from pathlib import Path

import pytest

from python.dict_meta_finder import DictMetaPathFinder


FIXTURES_DIR = Path(__file__).with_name("fixtures")


class MetaPathManager:
    """Context manager to temporarily install a meta path finder."""

    def __init__(self, finder):
        self.finder = finder

    def __enter__(self):
        sys.meta_path.insert(0, self.finder)
        return self.finder

    def __exit__(self, exc_type, exc, tb):
        if self.finder in sys.meta_path:
            sys.meta_path.remove(self.finder)
        return False


def clear_modules(prefix: str) -> None:
    for name in [m for m in sys.modules if m == prefix or m.startswith(f"{prefix}.")]:
        sys.modules.pop(name, None)


def copy_fixture_tree(tmp_path: Path) -> Path:
    root = tmp_path / "root"
    shutil.copytree(FIXTURES_DIR / "pkg", root / "pkg")
    return root


def test_loads_allowed_module(tmp_path: Path):
    clear_modules("pkg")
    tree = {"pkg": {"subpkg": {"mod": None}}}

    root = copy_fixture_tree(tmp_path)

    finder = DictMetaPathFinder(root, tree)

    with MetaPathManager(finder):
        pkg = importlib.import_module("pkg")
        subpkg = importlib.import_module("pkg.subpkg")
        mod = importlib.import_module("pkg.subpkg.mod")

    assert pkg.__file__ == str((root / "pkg" / "__init__.py").resolve())
    assert subpkg.value == "package"
    assert mod.value == "module"


@pytest.mark.parametrize(
    ("fullname", "message_fragment"),
    [
        ("pkg.subpkg.extra", "at"),
        ("pkg.unknown", "no matching file"),
        ("other", "no matching file"),
    ],
)
def test_disallowed_imports_raise(fullname: str, message_fragment: str, tmp_path: Path):
    clear_modules("pkg")
    root = copy_fixture_tree(tmp_path)

    finder = DictMetaPathFinder(root, {"pkg": {"subpkg": {"mod": None}}})

    with MetaPathManager(finder):
        with pytest.raises(ImportError) as err:
            importlib.import_module(fullname)

    assert message_fragment in str(err.value)


def test_missing_files_raise(tmp_path: Path):
    clear_modules("pkg")
    tree = {"pkg": {"subpkg": {"mod": None}}}
    root = copy_fixture_tree(tmp_path)
    (root / "pkg" / "subpkg" / "mod.py").unlink()

    finder = DictMetaPathFinder(root, tree)

    with MetaPathManager(finder):
        with pytest.raises(ImportError):
            importlib.import_module("pkg.subpkg.mod")
