use std::fs;
use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::TempDir;

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

#[test]
fn collects_transitive_imports_in_src_layout() {
    let project = TempDir::new().unwrap();

    write_file(
        &project.path().join("pyproject.toml"),
        "[project]\nname = \"demo\"\n",
    );

    let package_root = project.path().join("src/demo");

    write_file(&package_root.join("__init__.py"), "");
    write_file(
        &package_root.join("main.py"),
        "import demo.util\nfrom demo.subpkg import mod\nfrom . import shared\nfrom .subpkg import mod2\nfrom outside import nope\nfrom .subpkg.relative_pkg import nested\n",
    );
    write_file(&package_root.join("shared.py"), "");
    write_file(&package_root.join("util.py"), "from .subpkg import mod\n");

    let subpkg = package_root.join("subpkg");
    write_file(&subpkg.join("__init__.py"), "");
    write_file(
        &subpkg.join("mod.py"),
        "from .. import shared\nfrom ..other import more\n",
    );
    write_file(
        &subpkg.join("mod2.py"),
        "from ..subpkg.relative_pkg import nested\n",
    );

    let relative_pkg = subpkg.join("relative_pkg");
    write_file(&relative_pkg.join("__init__.py"), "");
    write_file(&relative_pkg.join("nested.py"), "from .. import mod\n");

    let other = package_root.join("other");
    write_file(&other.join("__init__.py"), "");
    write_file(&other.join("more.py"), "");

    let assert = Command::new(env!("CARGO_BIN_EXE_monolink"))
        .arg(project.path())
        .arg("demo.main")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let imports: Vec<&str> = stdout.lines().collect();

    assert_eq!(
        imports,
        vec![
            "demo.other.more",
            "demo.shared",
            "demo.subpkg.mod",
            "demo.subpkg.mod2",
            "demo.subpkg.relative_pkg.nested",
            "demo.util",
        ]
    );
}

#[test]
fn supports_in_place_layout_with_hyphenated_project_name() {
    let project = TempDir::new().unwrap();

    write_file(
        &project.path().join("pyproject.toml"),
        "[project]\nname = \"demo-tool\"\n",
    );

    let package_root = project.path().join("demo_tool");

    write_file(&package_root.join("__init__.py"), "");
    write_file(&package_root.join("app.py"), "from . import utils\n");

    let utils = package_root.join("utils");
    write_file(&utils.join("__init__.py"), "from .helper import inner\n");
    write_file(&utils.join("helper.py"), "from .nested import inner\n");

    write_file(&utils.join("nested.py"), "import demo_tool.core\n");

    write_file(&package_root.join("core.py"), "");

    let assert = Command::new(env!("CARGO_BIN_EXE_monolink"))
        .arg(project.path())
        .arg("demo_tool.app")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let imports: Vec<&str> = stdout.lines().collect();

    assert_eq!(
        imports,
        vec![
            "demo_tool.core",
            "demo_tool.utils",
            "demo_tool.utils.helper.inner",
            "demo_tool.utils.nested.inner",
        ]
    );
}
