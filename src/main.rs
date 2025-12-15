use std::collections::{BTreeSet, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Parser;
use ruff_python_ast::statement_visitor::{self, StatementVisitor};
use ruff_python_ast::{Alias, Identifier, Stmt};
use ruff_python_parser::parse_module;
use toml::Value;

#[derive(Debug, Parser)]
#[command(author, version, about = "Collect Ruff-parsed imports by package prefix", long_about = None)]
struct Args {
    /// Root directory of the Python project containing pyproject.toml
    project_root: PathBuf,

    /// Qualified module path (e.g., package.subpackage.module) to start traversal from
    module: String,
}

fn main() -> Result<()> {
    let Args {
        project_root,
        module,
    } = Args::parse();

    let pyproject = load_pyproject(&project_root)?;
    let package_prefix = infer_package_name(&pyproject)?;
    let source_roots = infer_source_roots(&project_root, &package_prefix, &pyproject)?;

    if !module.starts_with(&package_prefix) {
        bail!("Module '{module}' is not within the inferred package prefix '{package_prefix}'");
    }

    let mut imports = BTreeSet::new();
    let mut visited_modules = HashSet::new();
    let mut queue = VecDeque::from([module.clone()]);
    let mut monolink_files = BTreeSet::new();

    while let Some(current_module) = queue.pop_front() {
        if !visited_modules.insert(current_module.clone()) {
            continue;
        }

        let Some((path, source_root)) = module_file_path(&source_roots, &current_module) else {
            eprintln!(
                "Skipping {}: no module file found in inferred source roots",
                current_module
            );
            continue;
        };

        let context = match module_context(&source_root, &path) {
            Ok(context) => context,
            Err(err) => {
                eprintln!("Skipping {}: {err}", path.display());
                continue;
            }
        };

        if current_module == module && path.file_name().is_some_and(|name| name == "__init__.py") {
            if let Some(package_dir) = path.parent() {
                match collect_descendants(package_dir, &context.package_path) {
                    Ok(descendants) => {
                        for descendant in descendants {
                            if descendant != current_module {
                                if !visited_modules.contains(&descendant) {
                                    queue.push_back(descendant.clone());
                                }
                                imports.insert(descendant);
                            }
                        }
                    }
                    Err(err) => eprintln!("Skipping {} descendants: {err}", current_module),
                }
            }
        }

        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("Skipping {}: {err}", path.display());
                continue;
            }
        };

        let parsed = match parse_module(&source) {
            Ok(module) => module,
            Err(err) => {
                eprintln!("Failed to parse {}: {err}", path.display());
                continue;
            }
        };

        let mut next_modules = Vec::new();
        let mut collector = ImportCollector {
            package_prefix: &package_prefix,
            imports: &mut imports,
            package_path: &context.package_path,
            next_modules: &mut next_modules,
        };
        collector.visit_body(&parsed.syntax().body);

        record_monolink_files(&source_root, &context.package_path, &mut monolink_files);

        for next in next_modules {
            if !visited_modules.contains(&next) {
                queue.push_back(next);
            }
        }
    }

    for import in &imports {
        println!("{import}");
    }

    let mut dependencies = BTreeSet::new();
    for path in monolink_files {
        for dependency in load_monolink(&path)? {
            dependencies.insert(dependency);
        }
    }

    if !dependencies.is_empty() {
        println!();
        for dependency in dependencies {
            println!("{dependency}");
        }
    }

    Ok(())
}

fn load_pyproject(project_root: &Path) -> Result<Value> {
    let pyproject_path = project_root.join("pyproject.toml");
    let contents = fs::read_to_string(&pyproject_path)
        .with_context(|| format!("Failed to read {}", pyproject_path.display()))?;
    contents
        .parse::<Value>()
        .with_context(|| format!("Failed to parse {}", pyproject_path.display()))
}

fn infer_package_name(pyproject: &Value) -> Result<String> {
    let name = pyproject
        .get("project")
        .and_then(|project| project.get("name"))
        .and_then(Value::as_str)
        .or_else(|| {
            pyproject
                .get("tool")
                .and_then(|tool| tool.get("poetry"))
                .and_then(|poetry| poetry.get("name"))
                .and_then(Value::as_str)
        })
        .ok_or_else(|| anyhow::anyhow!("Missing project name in pyproject.toml"))?;

    Ok(name.replace('-', "_"))
}

fn infer_source_roots(
    project_root: &Path,
    package_name: &str,
    pyproject: &Value,
) -> Result<Vec<PathBuf>> {
    let mut roots: Vec<PathBuf> = Vec::new();

    if let Some(packages) = pyproject
        .get("tool")
        .and_then(|tool| tool.get("poetry"))
        .and_then(|poetry| poetry.get("packages"))
        .and_then(Value::as_array)
    {
        for package in packages.iter().filter_map(Value::as_table) {
            if package
                .get("include")
                .and_then(Value::as_str)
                .map(|include| include == package_name)
                .unwrap_or(false)
            {
                if let Some(from) = package.get("from").and_then(Value::as_str) {
                    roots.push(project_root.join(from));
                } else {
                    roots.push(project_root.to_path_buf());
                }
            }
        }
    }

    let src_root = project_root.join("src");
    if src_root.join(package_name).is_dir() {
        roots.push(src_root);
    }

    if project_root.join(package_name).is_dir() {
        roots.push(project_root.to_path_buf());
    }

    roots.sort();
    roots.dedup();

    if roots.is_empty() {
        bail!(
            "Could not determine source directories for package '{}' under {}",
            package_name,
            project_root.display()
        );
    }

    Ok(roots)
}

fn module_file_path(source_roots: &[PathBuf], module: &str) -> Option<(PathBuf, PathBuf)> {
    let relative: PathBuf = module.split('.').collect::<PathBuf>();

    for root in source_roots {
        let file_candidate = root.join(relative.with_extension("py"));
        if file_candidate.is_file() {
            return Some((file_candidate, root.clone()));
        }

        let package_candidate = root.join(&relative).join("__init__.py");
        if package_candidate.is_file() {
            return Some((package_candidate, root.clone()));
        }
    }

    None
}

fn collect_descendants(package_dir: &Path, package_path: &[String]) -> Result<Vec<String>> {
    let mut to_visit = VecDeque::from([(package_dir.to_path_buf(), package_path.to_vec())]);
    let mut modules = Vec::new();

    while let Some((dir, components)) = to_visit.pop_front() {
        let entries = fs::read_dir(&dir)
            .with_context(|| format!("Failed to read directory {}", dir.display()))?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                let mut subcomponents = components.clone();
                subcomponents.push(entry.file_name().to_string_lossy().into_owned());

                if path.join("__init__.py").is_file() {
                    modules.push(subcomponents.join("."));
                    to_visit.push_back((path, subcomponents));
                }
            } else if path
                .extension()
                .map(|ext| ext == "py")
                .unwrap_or(false)
                && path
                    .file_stem()
                    .is_some_and(|stem| stem != "__init__")
            {
                let mut module_components = components.clone();
                module_components.push(path.file_stem().unwrap().to_string_lossy().into_owned());
                modules.push(module_components.join("."));
            }
        }
    }

    Ok(modules)
}

struct ModuleContext {
    package_path: Vec<String>,
}

fn module_context(project_root: &Path, path: &Path) -> Result<ModuleContext> {
    let relative = path.strip_prefix(project_root).with_context(|| {
        format!(
            "{} is not inside {}",
            path.display(),
            project_root.display()
        )
    })?;

    let mut components: Vec<String> = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();

    if let Some(filename) = components.last_mut() {
        if let Some(stripped) = filename.strip_suffix(".py") {
            *filename = stripped.to_string();
        }
    }

    let is_init = components
        .last()
        .map(|last| last == "__init__")
        .unwrap_or(false);

    let mut package_start = 0;
    if components.len() > 1 {
        let mut current = project_root.to_path_buf();
        for (index, component) in components.iter().take(components.len() - 1).enumerate() {
            current.push(component);
            if current.join("__init__.py").exists() {
                package_start = index;
                break;
            }
        }
    }

    let mut module_path = components.split_off(package_start);
    if is_init {
        module_path.pop();
    }

    let package_path = if is_init {
        module_path.clone()
    } else {
        module_path[..module_path.len().saturating_sub(1)].to_vec()
    };

    Ok(ModuleContext { package_path })
}

fn record_monolink_files(
    source_root: &Path,
    package_path: &[String],
    monolink_files: &mut BTreeSet<PathBuf>,
) {
    let mut current = source_root.to_path_buf();
    for component in package_path {
        current.push(component);
        let candidate = current.join("monolink.toml");
        if candidate.is_file() {
            monolink_files.insert(candidate);
        }
    }
}

fn load_monolink(path: &Path) -> Result<Vec<String>> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let value: Value = contents
        .parse()
        .with_context(|| format!("Failed to parse {}", path.display()))?;

    let table = value
        .as_table()
        .ok_or_else(|| anyhow::anyhow!("monolink.toml must be a TOML table"))?;

    for key in table.keys() {
        if key != "dependencies" {
            bail!("Unsupported key '{key}' in {}", path.display());
        }
    }

    let dependencies = match table.get("dependencies") {
        Some(value) => {
            let deps = value
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("[dependencies] must be an array"))?;

            let mut parsed = Vec::new();
            for item in deps {
                parsed.push(
                    item.as_str()
                        .ok_or_else(|| anyhow::anyhow!("dependencies entries must be strings"))?
                        .to_string(),
                );
            }
            parsed
        }
        None => Vec::new(),
    };

    Ok(dependencies)
}

struct ImportCollector<'a> {
    package_prefix: &'a str,
    imports: &'a mut BTreeSet<String>,
    package_path: &'a [String],
    next_modules: &'a mut Vec<String>,
}

impl<'a> ImportCollector<'a> {
    fn push_import(&mut self, alias: &Alias, base: Option<&str>) {
        let name = alias.name.id.to_string();
        let import = if let Some(base) = base {
            if name == "*" {
                base.to_string()
            } else if base.is_empty() {
                name
            } else {
                format!("{base}.{name}")
            }
        } else {
            name
        };

        if import.starts_with(self.package_prefix) {
            self.next_modules.push(import.clone());
            self.imports.insert(import);
        }
    }

    fn resolve_import_from(&self, module: Option<&Identifier>, level: u32) -> Option<String> {
        let level = level as usize;
        if level == 0 {
            return module.map(|module| module.id.to_string());
        }

        let package_len = self.package_path.len();
        let relative_hops = level.saturating_sub(1);
        if relative_hops > package_len {
            return None;
        }

        let cutoff = package_len.saturating_sub(relative_hops);
        let mut parts = self.package_path[..cutoff].to_vec();
        if let Some(module) = module {
            parts.extend(module.id.to_string().split('.').map(String::from));
        }

        Some(parts.join("."))
    }
}

impl<'a> StatementVisitor<'a> for ImportCollector<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::Import(stmt_import) => {
                for alias in &stmt_import.names {
                    self.push_import(alias, None);
                }
            }
            Stmt::ImportFrom(stmt_import_from) => {
                let base = self
                    .resolve_import_from(stmt_import_from.module.as_ref(), stmt_import_from.level);
                if let Some(ref base) = base {
                    if base.starts_with(self.package_prefix) {
                        self.next_modules.push(base.clone());
                    }

                    for alias in &stmt_import_from.names {
                        self.push_import(alias, Some(base));
                    }
                }
            }
            _ => statement_visitor::walk_stmt(self, stmt),
        }
    }
}

