# monolink

A small utility binary that scans a Python project using Ruff's AST parser to
list imports beginning with the package defined in `pyproject.toml`.

## Usage

```bash
cargo run -- <project_root> <package.module>
```

Provide the root directory containing `pyproject.toml` and the fully qualified
module path to start from (e.g., `my_package.app.main`). The tool infers the
package name from the project metadata (e.g., `[project].name` or
`[tool.poetry].name`), locates the source tree using standard conventions
(`src/` layout or in-place package), resolves relative imports to their
absolute form, walks imports transitively starting from the given module, and
prints a sorted, deduplicated list of matching import paths.
