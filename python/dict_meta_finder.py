"""A MetaPathFinder that validates imports against a declared module tree.

The finder constrains imports to a pre-defined tree of packages/modules and
loads the corresponding Python files from a configured root directory.
"""

from __future__ import annotations

import importlib.abc
import importlib.util
from importlib.machinery import ModuleSpec, SourceFileLoader
from pathlib import Path
from typing import Mapping, NoReturn, Optional, TypeAlias

ModuleTree: TypeAlias = Mapping[str, "ModuleTree | None"]


class DictMetaPathFinder(importlib.abc.MetaPathFinder):
    """Restrict imports to a declared tree of modules.

    Args:
        root_directory: Filesystem root where the package/module files reside.
        module_tree: Nested mapping declaring the allowed import graph. A ``None``
            leaf indicates a module (``module.py``). A mapping leaf indicates a
            package and may contain further children.

    Any import not present in the ``module_tree`` raises ``ImportError``. Valid
    imports are resolved to ``root_directory`` and loaded from disk.
    """

    def __init__(self, root_directory: Path | str, module_tree: ModuleTree) -> None:
        self._root = Path(root_directory).resolve()
        self._module_tree = self._validate_tree(module_tree)

    def find_spec(
        self, fullname: str, path: Optional[str], target: object | None = None
    ) -> ModuleSpec | None:
        node, is_package = self._resolve(fullname)
        module_path = self._path_for(fullname, is_package)

        if not module_path.exists():
            raise ImportError(
                f"Validated module '{fullname}' is missing at {module_path!s}"
            )

        loader = SourceFileLoader(fullname, str(module_path))
        submodule_locations = [str(module_path.parent)] if is_package else None
        return importlib.util.spec_from_file_location(
            fullname,
            module_path,
            loader=loader,
            submodule_search_locations=submodule_locations,
        )

    def _resolve(self, fullname: str) -> tuple[ModuleTree | None, bool]:
        parts = fullname.split(".")
        node: ModuleTree | None = self._module_tree

        for index, part in enumerate(parts):
            if not isinstance(node, Mapping) or part not in node:
                self._raise_not_permitted(fullname)

            node = node[part]

        is_package = isinstance(node, Mapping)
        return node, is_package

    def _path_for(self, fullname: str, is_package: bool) -> Path:
        parts = fullname.split(".")
        base = self._root.joinpath(*parts)
        return base / "__init__.py" if is_package else base.with_suffix(".py")

    @staticmethod
    def _validate_tree(tree: ModuleTree) -> ModuleTree:
        for key, value in tree.items():
            if value is None:
                continue
            if isinstance(value, Mapping):
                DictMetaPathFinder._validate_tree(value)
            else:
                raise TypeError(
                    "Module tree values must be nested mappings or None for modules"
                )
        return tree

    def _raise_not_permitted(self, fullname: str) -> NoReturn:
        path_parts = fullname.split(".")
        base_path = self._root.joinpath(*path_parts)
        module_file = base_path.with_suffix(".py")
        package_file = base_path / "__init__.py"

        if module_file.exists():
            location_note = f" at {module_file!s}"
        elif package_file.exists():
            location_note = f" at {package_file!s}"
        else:
            location_note = " and no matching file was found on disk"

        raise ImportError(
            f"Module '{fullname}' is not permitted by finder{location_note}"
        )
