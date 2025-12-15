use std::process::Command;

use assert_cmd::prelude::*;
use std::path::PathBuf;

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(path)
}

#[test]
fn collects_transitive_imports_in_src_layout() {
    let assert = Command::new(env!("CARGO_BIN_EXE_monolink"))
        .arg(fixture("src_layout"))
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
    let assert = Command::new(env!("CARGO_BIN_EXE_monolink"))
        .arg(fixture("in_place_layout"))
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

#[test]
fn collects_monolink_dependencies_across_packages() {
    let assert = Command::new(env!("CARGO_BIN_EXE_monolink"))
        .arg(fixture("monolink"))
        .arg("demo.main")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let imports: Vec<&str> = stdout.lines().collect();

    assert_eq!(
        imports,
        vec![
            "demo.shared",
            "demo.subpkg.helper",
            "demo.subpkg.leaf",
            "demo.subpkg.mod",
            "",
            "numpy",
            "pandas<3",
            "pydantic>=1.10",
            "requests",
        ]
    );
}

#[test]
fn recursively_collects_descendants_when_entry_is_package() {
    let assert = Command::new(env!("CARGO_BIN_EXE_monolink"))
        .arg(fixture("package_arg"))
        .arg("demo")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let imports: Vec<&str> = stdout.lines().collect();

    assert_eq!(
        imports,
        vec![
            "demo.a",
            "demo.subpkg",
            "demo.subpkg.b",
            "demo.subpkg.nested",
            "demo.subpkg.nested.c",
        ]
    );
}
