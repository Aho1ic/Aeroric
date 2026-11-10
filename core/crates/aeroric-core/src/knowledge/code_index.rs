//! R4-1 (2026-11-10): a project's code, read structurally, for the Wiki's
//! module pages. Modules are two levels deep (a package, then its first
//! level: a Rust crate's top `mod`, a Swift target's first folder, a
//! Python sub-package, a JS/TS package's first folder); symbols and imports
//! come from tree-sitter, so a dependency edge is only ever one an import
//! states (EXTRACTED). Anything inferred is R4-2's, from a model, marked so.
//!
//! Files come from `git ls-files` (so `.gitignore` holds) or, outside git, a
//! walk that skips build and dependency folders. Each file's version is its
//! git blob hash, computed from what is on disk.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use sha1::{Digest, Sha1};

/// A project with more modules than this has its packages folded into one
/// page each, largest first, until it fits.
pub const MAX_MODULES: usize = 60;
const MAX_FILES: usize = 8000;
const MAX_FILE_BYTES: u64 = 512 * 1024;
/// Symbols a module page lists.
pub const MAX_SYMBOLS: usize = 20;
/// Skipped by the walk outside git.
const SKIP_DIRS: [&str; 12] = [
    ".git",
    "target",
    "node_modules",
    "build",
    "dist",
    ".build",
    ".venv",
    "venv",
    "__pycache__",
    "DerivedData",
    ".artifacts",
    "Pods",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lang {
    Rust,
    Swift,
    Python,
    TypeScript,
    Tsx,
    /// Read as a file of the module only: no symbols, no imports.
    Other,
}

impl Lang {
    fn of(path: &str) -> Option<Lang> {
        let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
        Some(match ext.as_str() {
            "rs" => Lang::Rust,
            "swift" => Lang::Swift,
            "py" => Lang::Python,
            "ts" | "mts" | "cts" | "js" | "mjs" | "cjs" => Lang::TypeScript,
            "tsx" | "jsx" => Lang::Tsx,
            "go" | "java" | "kt" | "kts" | "c" | "h" | "cc" | "cpp" | "hpp" | "m" | "mm" | "cs"
            | "rb" | "php" | "scala" | "dart" | "lua" | "zig" => Lang::Other,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Lang::Rust => "Rust",
            Lang::Swift => "Swift",
            Lang::Python => "Python",
            Lang::TypeScript => "TypeScript/JavaScript",
            Lang::Tsx => "TSX/JSX",
            Lang::Other => "其他",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    /// `struct`, `enum`, `trait`, `fn`, `class`, `protocol`, `interface`, …
    pub kind: String,
    pub path: String,
    pub line: u32,
}

impl Symbol {
    fn is_type(&self) -> bool {
        !matches!(self.kind.as_str(), "fn" | "func" | "def" | "function")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeFile {
    pub path: String,
    /// The git blob hash of what is on disk.
    pub blob: String,
    pub lines: u32,
    pub lang: Lang,
}

/// An import that names another module, where it was read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    pub to: String,
    pub path: String,
    pub line: u32,
    /// The import as written, trimmed.
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    /// `<package dir>` or `<package dir>::<first level>`; unique.
    pub id: String,
    pub package: String,
    /// The package's own name (crate, target, package.json `name`, …).
    pub package_name: String,
    /// The first level's name; `None` for the package's own files.
    pub name: Option<String>,
    pub dir: String,
    pub files: Vec<CodeFile>,
    pub entries: Vec<String>,
    pub symbols: Vec<Symbol>,
    /// One edge per module depended on: the first import read.
    pub deps: Vec<Edge>,
}

impl Module {
    pub fn title(&self) -> String {
        match &self.name {
            Some(name) => format!("{} · {name}", self.package_name),
            None => self.package_name.clone(),
        }
    }

    pub fn lines(&self) -> u32 {
        self.files.iter().map(|f| f.lines).sum()
    }

    pub fn langs(&self) -> BTreeSet<Lang> {
        self.files.iter().map(|f| f.lang).collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeIndex {
    /// `git rev-parse HEAD`; `None` outside git.
    pub commit: Option<String>,
    pub modules: Vec<Module>,
    /// Modules left out past [`MAX_MODULES`] even after folding.
    pub omitted: usize,
    /// Whether the project folder could be read at all.
    pub available: bool,
}

pub fn blob_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(format!("blob {}\0", bytes.len()).as_bytes());
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C").arg(dir).args(args);
    crate::runtime::subprocess::configure_background_command(&mut cmd);
    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Relative paths of the project's files that could be source or a
/// package manifest.
fn list_files(dir: &Path) -> (Vec<String>, bool) {
    // Third-party code kept in the repository is not the project's.
    let vendored = |path: &str| {
        path.split('/')
            .any(|part| matches!(part, "vendor" | "third_party" | "third-party" | "external"))
    };
    let wanted = |path: &str| {
        !vendored(path)
            && (Lang::of(path).is_some()
                || matches!(
                    path.rsplit('/').next().unwrap_or(path),
                    "Cargo.toml" | "Package.swift" | "package.json"
                ))
    };
    if let Some(out) = git(dir, &["ls-files", "-co", "--exclude-standard", "-z"]) {
        let mut files: Vec<String> = out
            .split('\0')
            .filter(|p| !p.is_empty() && wanted(p))
            .map(str::to_string)
            .collect();
        files.sort();
        files.truncate(MAX_FILES);
        return (files, true);
    }
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                    stack.push(path);
                }
            } else if let Ok(rel) = path.strip_prefix(dir) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                if wanted(&rel) {
                    files.push(rel);
                }
            }
            if files.len() >= MAX_FILES {
                break;
            }
        }
    }
    files.sort();
    (files, false)
}

/// Changes when a file is added, removed or written: paths, sizes and
/// modification times, no reading.
pub fn fingerprint(dir: &Path) -> String {
    let (files, _) = list_files(dir);
    let mut hasher = Sha1::new();
    for rel in &files {
        hasher.update(rel.as_bytes());
        if let Ok(meta) = std::fs::metadata(dir.join(rel)) {
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis())
                .unwrap_or(0);
            hasher.update(format!("|{}|{modified}\n", meta.len()).as_bytes());
        }
    }
    hasher.finalize()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Manifest {
    Cargo,
    Swift,
    Node,
    Python,
}

#[derive(Clone, Debug)]
struct Package {
    dir: String,
    name: String,
    manifest: Manifest,
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

fn join(dir: &str, rest: &str) -> String {
    if dir.is_empty() {
        rest.to_string()
    } else {
        format!("{dir}/{rest}")
    }
}

/// `name = "x"` in a manifest's `[package]` / `"name": "x"`.
fn manifest_name(text: &str, manifest: Manifest) -> Option<String> {
    match manifest {
        Manifest::Cargo => {
            let mut in_package = false;
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with('[') {
                    in_package = line == "[package]";
                } else if in_package {
                    if let Some(rest) = line.strip_prefix("name") {
                        let value = rest.trim_start().strip_prefix('=')?.trim();
                        return Some(value.trim_matches('"').to_string());
                    }
                }
            }
            None
        }
        Manifest::Node => serde_json::from_str::<serde_json::Value>(text)
            .ok()?
            .get("name")?
            .as_str()
            .map(str::to_string),
        _ => None,
    }
}

fn packages(dir: &Path, files: &[String]) -> Vec<Package> {
    let mut out = Vec::new();
    let file_set: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    for rel in files {
        let file = rel.rsplit('/').next().unwrap_or(rel);
        let manifest = match file {
            "Cargo.toml" => Manifest::Cargo,
            "Package.swift" => Manifest::Swift,
            "package.json" => Manifest::Node,
            _ => continue,
        };
        let pdir = parent(rel).to_string();
        let text = std::fs::read_to_string(dir.join(rel)).unwrap_or_default();
        let name = match manifest {
            // A workspace-only Cargo.toml is not a crate.
            Manifest::Cargo => match manifest_name(&text, manifest) {
                Some(name) => name,
                None => continue,
            },
            Manifest::Node => manifest_name(&text, manifest).unwrap_or_else(|| dir_name(&pdir)),
            _ => dir_name(&pdir),
        };
        out.push(Package {
            dir: pdir,
            name,
            manifest,
        });
    }
    // Top Python packages: a folder with `__init__.py` whose parent has none.
    for rel in files.iter().filter(|r| r.ends_with("/__init__.py")) {
        let pdir = parent(rel);
        let up = parent(pdir);
        if !file_set.contains(join(up, "__init__.py").as_str()) {
            out.push(Package {
                dir: pdir.to_string(),
                name: dir_name(pdir),
                manifest: Manifest::Python,
            });
        }
    }
    out
}

fn dir_name(dir: &str) -> String {
    if dir.is_empty() {
        "（根目录）".into()
    } else {
        dir.rsplit('/').next().unwrap_or(dir).to_string()
    }
}

/// The package a file belongs to (the deepest one holding it) and its
/// first-level module there.
fn place(rel: &str, packages: &[Package]) -> (Package, Option<String>) {
    let holder = packages
        .iter()
        .filter(|p| p.dir.is_empty() || rel.starts_with(&format!("{}/", p.dir)))
        .max_by_key(|p| p.dir.len())
        .cloned();
    let Some(package) = holder else {
        // Outside any package: the top folder is the package, the next one
        // the module.
        let parts: Vec<&str> = rel.split('/').collect();
        let package = Package {
            dir: if parts.len() > 1 {
                parts[0].to_string()
            } else {
                String::new()
            },
            name: if parts.len() > 1 {
                parts[0].to_string()
            } else {
                dir_name("")
            },
            manifest: Manifest::Node,
        };
        let name = (parts.len() > 2).then(|| parts[1].to_string());
        return (package, name);
    };
    let inner = if package.dir.is_empty() {
        rel
    } else {
        &rel[package.dir.len() + 1..]
    };
    let parts: Vec<&str> = inner.split('/').collect();
    let name = match package.manifest {
        Manifest::Cargo => match parts.as_slice() {
            ["src", file] if matches!(*file, "lib.rs" | "main.rs") => None,
            ["src", file] => Some(file.trim_end_matches(".rs").to_string()),
            ["src", sub, ..] => Some(sub.to_string()),
            [_] => None,
            [first, ..] => Some(first.to_string()),
            [] => None,
        },
        Manifest::Swift => match parts.as_slice() {
            ["Sources" | "Tests", target, _] => Some(target.to_string()),
            ["Sources" | "Tests", target, sub, ..] => Some(format!("{target}/{sub}")),
            [_] => None,
            [first, ..] => Some(first.to_string()),
            [] => None,
        },
        Manifest::Python => match parts.as_slice() {
            [_] | [] => None,
            [sub, ..] => Some(sub.to_string()),
        },
        Manifest::Node => {
            let parts = match parts.first() {
                Some(&"src") if parts.len() > 1 => &parts[1..],
                _ => &parts[..],
            };
            (parts.len() > 1).then(|| parts[0].to_string())
        }
    };
    (package, name)
}

fn module_id(package: &Package, name: Option<&str>) -> String {
    let dir = if package.dir.is_empty() {
        "."
    } else {
        &package.dir
    };
    match name {
        Some(name) => format!("{dir}::{name}"),
        None => dir.to_string(),
    }
}

/// What one file says: its symbols and the imports it states.
#[derive(Default)]
struct Parsed {
    symbols: Vec<Symbol>,
    imports: Vec<(String, u32)>,
}

fn text<'a>(node: tree_sitter::Node, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or("")
}

fn named_children<'t>(node: tree_sitter::Node<'t>) -> Vec<tree_sitter::Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn parse(lang: Lang, path: &str, src: &[u8]) -> Parsed {
    let language: tree_sitter::Language = match lang {
        Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
        Lang::Swift => tree_sitter_swift::LANGUAGE.into(),
        Lang::Python => tree_sitter_python::LANGUAGE.into(),
        Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Lang::Other => return Parsed::default(),
    };
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&language).is_err() {
        return Parsed::default();
    }
    let Some(tree) = parser.parse(src, None) else {
        return Parsed::default();
    };
    let mut out = Parsed::default();
    for node in named_children(tree.root_node()) {
        match lang {
            Lang::Rust => rust_item(node, src, path, &mut out),
            Lang::Swift => swift_item(node, src, path, &mut out),
            Lang::Python => python_item(node, src, path, &mut out),
            Lang::TypeScript | Lang::Tsx => ts_item(node, src, path, &mut out),
            Lang::Other => {}
        }
    }
    out
}

fn push_symbol(out: &mut Parsed, name: &str, kind: &str, path: &str, line: u32) {
    if !name.is_empty() {
        out.symbols.push(Symbol {
            name: name.to_string(),
            kind: kind.to_string(),
            path: path.to_string(),
            line,
        });
    }
}

fn row(node: tree_sitter::Node) -> u32 {
    node.start_position().row as u32 + 1
}

fn rust_item(node: tree_sitter::Node, src: &[u8], path: &str, out: &mut Parsed) {
    let kind = match node.kind() {
        "struct_item" => "struct",
        "enum_item" => "enum",
        "trait_item" => "trait",
        "union_item" => "union",
        "type_item" => "type",
        "function_item" => "fn",
        "use_declaration" => {
            let arg = node
                .child_by_field_name("argument")
                .map(|a| text(a, src))
                .unwrap_or("");
            out.imports.push((arg.to_string(), row(node)));
            return;
        }
        _ => return,
    };
    let public = named_children(node)
        .iter()
        .any(|c| c.kind() == "visibility_modifier" && text(*c, src) == "pub");
    if let (true, Some(name)) = (public, node.child_by_field_name("name")) {
        push_symbol(out, text(name, src), kind, path, row(node));
    }
}

fn swift_item(node: tree_sitter::Node, src: &[u8], path: &str, out: &mut Parsed) {
    let kind = match node.kind() {
        "import_declaration" => {
            let name = named_children(node)
                .into_iter()
                .find(|c| c.kind() == "identifier")
                .map(|c| text(c, src).to_string())
                .unwrap_or_default();
            out.imports.push((name, row(node)));
            return;
        }
        "protocol_declaration" => "protocol".to_string(),
        "class_declaration" => {
            let mut cursor = node.walk();
            let keyword = node
                .children(&mut cursor)
                .find(|c| {
                    matches!(
                        c.kind(),
                        "class" | "struct" | "enum" | "extension" | "actor"
                    )
                })
                .map(|c| c.kind().to_string());
            match keyword {
                Some(k) if k != "extension" => k,
                _ => return,
            }
        }
        "typealias_declaration" => "typealias".to_string(),
        "function_declaration" => "func".to_string(),
        _ => return,
    };
    let private = named_children(node)
        .iter()
        .any(|c| c.kind() == "modifiers" && text(*c, src).contains("private"));
    if private {
        return;
    }
    if let Some(name) = node.child_by_field_name("name") {
        push_symbol(out, text(name, src), &kind, path, row(node));
    }
}

fn python_item(node: tree_sitter::Node, src: &[u8], path: &str, out: &mut Parsed) {
    let node = if node.kind() == "decorated_definition" {
        match node.child_by_field_name("definition") {
            Some(d) => d,
            None => return,
        }
    } else {
        node
    };
    match node.kind() {
        "class_definition" | "function_definition" => {
            let kind = if node.kind() == "class_definition" {
                "class"
            } else {
                "def"
            };
            if let Some(name) = node.child_by_field_name("name") {
                let name = text(name, src);
                if !name.starts_with('_') {
                    push_symbol(out, name, kind, path, row(node));
                }
            }
        }
        "import_statement" => {
            for c in named_children(node) {
                let target = match c.kind() {
                    "dotted_name" => Some(c),
                    "aliased_import" => c.child_by_field_name("name"),
                    _ => None,
                };
                if let Some(t) = target {
                    out.imports.push((text(t, src).to_string(), row(node)));
                }
            }
        }
        "import_from_statement" => {
            if let Some(m) = node.child_by_field_name("module_name") {
                out.imports.push((text(m, src).to_string(), row(node)));
            }
        }
        _ => {}
    }
}

fn ts_item(node: tree_sitter::Node, src: &[u8], path: &str, out: &mut Parsed) {
    match node.kind() {
        "import_statement" => {
            if let Some(source) = node.child_by_field_name("source") {
                let spec = text(source, src).trim_matches(['"', '\'', '`']).to_string();
                out.imports.push((spec, row(node)));
            }
        }
        "export_statement" => {
            let Some(decl) = node.child_by_field_name("declaration") else {
                // `export … from '…'` re-exports read like imports.
                if let Some(source) = node.child_by_field_name("source") {
                    let spec = text(source, src).trim_matches(['"', '\'', '`']).to_string();
                    out.imports.push((spec, row(node)));
                }
                return;
            };
            let kind = match decl.kind() {
                "class_declaration" | "abstract_class_declaration" => "class",
                "interface_declaration" => "interface",
                "type_alias_declaration" => "type",
                "enum_declaration" => "enum",
                "function_declaration" | "generator_function_declaration" => "function",
                _ => return,
            };
            if let Some(name) = decl.child_by_field_name("name") {
                push_symbol(out, text(name, src), kind, path, row(decl));
            }
        }
        _ => {}
    }
}

/// Reads and indexes the project at `dir`.
pub fn scan(dir: &Path) -> CodeIndex {
    if !dir.is_dir() {
        return CodeIndex::default();
    }
    let (files, in_git) = list_files(dir);
    let commit = in_git
        .then(|| git(dir, &["rev-parse", "HEAD"]))
        .flatten()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let packages = packages(dir, &files);
    let mut modules: BTreeMap<String, Module> = BTreeMap::new();
    // module id → its files' imports, resolved once every module is known.
    let mut imports: Vec<(String, Lang, String, Vec<(String, u32)>, Package)> = Vec::new();
    for rel in &files {
        // The manifest is the package's, not a module's file.
        if rel.ends_with("Package.swift") {
            continue;
        }
        let Some(lang) = Lang::of(rel) else {
            continue;
        };
        let full = dir.join(rel);
        let Ok(meta) = std::fs::metadata(&full) else {
            continue;
        };
        if meta.len() > MAX_FILE_BYTES {
            continue;
        }
        let Ok(bytes) = std::fs::read(&full) else {
            continue;
        };
        let (package, name) = place(rel, &packages);
        let id = module_id(&package, name.as_deref());
        let parsed = parse(lang, rel, &bytes);
        let module = modules.entry(id.clone()).or_insert_with(|| Module {
            id: id.clone(),
            package: package.dir.clone(),
            package_name: package.name.clone(),
            name: name.clone(),
            dir: match &name {
                Some(n) => module_dir(&package, n),
                None => package.dir.clone(),
            },
            files: Vec::new(),
            entries: Vec::new(),
            symbols: Vec::new(),
            deps: Vec::new(),
        });
        let file = rel.rsplit('/').next().unwrap_or(rel);
        if matches!(
            file,
            "lib.rs"
                | "main.rs"
                | "mod.rs"
                | "__init__.py"
                | "__main__.py"
                | "index.ts"
                | "index.tsx"
                | "index.js"
                | "main.swift"
                | "Package.swift"
        ) || (lang == Lang::Rust && name.as_deref().is_some_and(|n| file == format!("{n}.rs")))
        {
            module.entries.push(rel.clone());
        }
        module.files.push(CodeFile {
            path: rel.clone(),
            blob: blob_hash(&bytes),
            lines: bytecount_lines(&bytes),
            lang,
        });
        module.symbols.extend(parsed.symbols);
        imports.push((id, lang, rel.clone(), parsed.imports, package));
    }
    let mut index = CodeIndex {
        commit,
        modules: Vec::new(),
        omitted: 0,
        available: true,
    };
    let mut modules: Vec<Module> = modules.into_values().collect();
    // A module folded away answers for the page it went into.
    let mut aliases: HashMap<String, String> = HashMap::new();
    fold(&mut modules, &mut index.omitted, &mut aliases);
    let ids: BTreeSet<String> = modules.iter().map(|m| m.id.clone()).collect();
    // A file's module after folding: the module whose files hold it.
    let owner: HashMap<String, String> = modules
        .iter()
        .flat_map(|m| m.files.iter().map(|f| (f.path.clone(), m.id.clone())))
        .collect();
    let resolver = Resolver::new(&packages, &ids, &aliases, &files);
    let mut deps: HashMap<String, BTreeMap<String, Edge>> = HashMap::new();
    for (_, lang, rel, list, package) in imports {
        let Some(from) = owner.get(&rel) else {
            continue;
        };
        for (spec, line) in list {
            let Some(to) = resolver.resolve(lang, &rel, &spec, &package) else {
                continue;
            };
            if &to == from {
                continue;
            }
            deps.entry(from.clone())
                .or_default()
                .entry(to.clone())
                .or_insert(Edge {
                    to,
                    path: rel.clone(),
                    line,
                    text: spec.trim().chars().take(120).collect(),
                });
        }
    }
    for module in &mut modules {
        module.deps = deps
            .remove(&module.id)
            .map(|m| m.into_values().collect())
            .unwrap_or_default();
        // Types first, in file order; then functions.
        module.symbols.sort_by_key(|s| !s.is_type());
        module.entries.sort();
    }
    index.modules = modules;
    index
}

fn module_dir(package: &Package, name: &str) -> String {
    let inner = match package.manifest {
        Manifest::Cargo => format!("src/{name}"),
        Manifest::Swift => format!("Sources/{name}"),
        Manifest::Node => name.to_string(),
        Manifest::Python => name.to_string(),
    };
    join(&package.dir, &inner)
}

fn bytecount_lines(bytes: &[u8]) -> u32 {
    let n = bytes.iter().filter(|b| **b == b'\n').count() as u32;
    if bytes.last().is_some_and(|b| *b != b'\n') {
        n + 1
    } else {
        n
    }
}

/// Several modules as one: their files, entry points and symbols.
fn merged(mut into: Vec<Module>, id: String, name: Option<String>, dir: String) -> Module {
    into.sort_by(|a, b| a.id.cmp(&b.id));
    let mut out = into[0].clone();
    out.id = id;
    out.name = name;
    out.dir = dir;
    out.files.clear();
    out.entries.clear();
    out.symbols.clear();
    for m in into {
        out.files.extend(m.files);
        out.entries.extend(m.entries);
        out.symbols.extend(m.symbols);
    }
    out
}

/// Past [`MAX_MODULES`]:
/// 1. folds whole packages of at most half that many modules into one page
///    each, the one whose modules are smallest on average first;
/// 2. then, one at a time, moves the smallest module anywhere into its
///    package's "其他" page;
/// 3. and if it still does not fit, keeps the largest.
///
/// Real data (2026-11-10): folding the package with the most modules first
/// turned the main crate into a single page.
fn fold(modules: &mut Vec<Module>, omitted: &mut usize, aliases: &mut HashMap<String, String>) {
    const OTHER: &str = "其他";
    while modules.len() > MAX_MODULES {
        let mut sizes: BTreeMap<String, (usize, u64)> = BTreeMap::new();
        for m in modules.iter() {
            let entry = sizes.entry(m.package.clone()).or_default();
            entry.0 += 1;
            entry.1 += u64::from(m.lines());
        }
        let Some((package, _)) = sizes
            .into_iter()
            .filter(|(_, (count, _))| *count > 1 && *count <= MAX_MODULES / 2)
            .min_by_key(|(_, (count, lines))| lines / *count as u64)
        else {
            break;
        };
        let (into, rest): (Vec<Module>, Vec<Module>) =
            modules.drain(..).partition(|m| m.package == package);
        *modules = rest;
        let id = if package.is_empty() {
            ".".to_string()
        } else {
            package.clone()
        };
        for m in &into {
            aliases.insert(m.id.clone(), id.clone());
        }
        modules.push(merged(into, id, None, package));
    }
    // One merge at a time: the smallest module anywhere joins its package's
    // "其他" page, so pages go to where the code is. Merging a package's k
    // smallest at once emptied the Swift package; always merging in the
    // package with the most modules evened every package out to the same
    // page count, whatever its size (real data, 2026-11-10).
    while modules.len() > MAX_MODULES {
        let is_other = |m: &Module| m.name.as_deref() == Some(OTHER);
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for m in modules.iter().filter(|m| !is_other(m)) {
            *counts.entry(m.package.clone()).or_default() += 1;
        }
        let has_other = |package: &str| modules.iter().any(|m| m.package == package && is_other(m));
        let Some(smallest) = (0..modules.len())
            .filter(|i| !is_other(&modules[*i]))
            .filter(|i| {
                let package = &modules[*i].package;
                has_other(package) || counts.get(package).copied().unwrap_or(0) >= 2
            })
            .min_by_key(|i| (modules[*i].lines(), modules[*i].id.clone()))
        else {
            break;
        };
        let package = modules[smallest].package.clone();
        let mut chosen: BTreeSet<usize> = [smallest].into();
        match modules
            .iter()
            .position(|m| m.package == package && is_other(m))
        {
            Some(other) => {
                chosen.insert(other);
            }
            None => {
                let partner = (0..modules.len())
                    .filter(|i| *i != smallest && modules[*i].package == package)
                    .min_by_key(|i| (modules[*i].lines(), modules[*i].id.clone()));
                chosen.extend(partner);
            }
        }
        let mut into = Vec::new();
        let mut rest = Vec::new();
        for (i, m) in modules.drain(..).enumerate() {
            if chosen.contains(&i) {
                into.push(m);
            } else {
                rest.push(m);
            }
        }
        *modules = rest;
        let template = into[0].clone();
        let package_ref = Package {
            dir: template.package.clone(),
            name: template.package_name.clone(),
            manifest: Manifest::Node,
        };
        let id = module_id(&package_ref, Some(OTHER));
        for m in &into {
            aliases.insert(m.id.clone(), id.clone());
        }
        modules.push(merged(
            into,
            id,
            Some(OTHER.into()),
            template.package.clone(),
        ));
    }
    if modules.len() > MAX_MODULES {
        modules.sort_by_key(|m| std::cmp::Reverse(m.lines()));
        *omitted = modules.len() - MAX_MODULES;
        modules.truncate(MAX_MODULES);
    }
    modules.sort_by(|a, b| a.id.cmp(&b.id));
}

/// Turns an import into the module it names, if it is in this project.
struct Resolver<'a> {
    ids: &'a BTreeSet<String>,
    aliases: &'a HashMap<String, String>,
    /// Rust crate name (`-` as `_`) and Swift target → package dir.
    crates: HashMap<String, &'a Package>,
    swift_targets: HashMap<String, &'a Package>,
    node: HashMap<String, &'a Package>,
    python: HashMap<String, &'a Package>,
    files: BTreeSet<&'a str>,
}

impl<'a> Resolver<'a> {
    fn new(
        packages: &'a [Package],
        ids: &'a BTreeSet<String>,
        aliases: &'a HashMap<String, String>,
        files: &'a [String],
    ) -> Self {
        let mut r = Resolver {
            ids,
            aliases,
            crates: HashMap::new(),
            swift_targets: HashMap::new(),
            node: HashMap::new(),
            python: HashMap::new(),
            files: files.iter().map(String::as_str).collect(),
        };
        for p in packages {
            match p.manifest {
                Manifest::Cargo => {
                    r.crates.insert(p.name.replace('-', "_"), p);
                }
                Manifest::Node => {
                    r.node.insert(p.name.clone(), p);
                }
                Manifest::Python => {
                    r.python.insert(p.name.clone(), p);
                }
                Manifest::Swift => {}
            }
        }
        for rel in files {
            for root in ["Sources/", "Tests/"] {
                let Some(pos) = rel.find(root) else { continue };
                let after = &rel[pos + root.len()..];
                let Some((target, _)) = after.split_once('/') else {
                    continue;
                };
                let pdir = rel[..pos].trim_end_matches('/');
                if let Some(p) = packages
                    .iter()
                    .find(|p| p.manifest == Manifest::Swift && p.dir == pdir)
                {
                    r.swift_targets.entry(target.to_string()).or_insert(p);
                }
            }
        }
        r
    }

    /// The package's module `name`, else the package itself, if either is
    /// a module.
    /// `id`, or the page it was folded into (following folds of folds).
    fn current(&self, id: String) -> Option<String> {
        let mut id = id;
        for _ in 0..8 {
            if self.ids.contains(&id) {
                return Some(id);
            }
            id = self.aliases.get(&id)?.clone();
        }
        None
    }

    fn pick(&self, package: &Package, name: Option<&str>) -> Option<String> {
        if let Some(name) = name {
            if let Some(id) = self.current(module_id(package, Some(name))) {
                return Some(id);
            }
        }
        self.current(module_id(package, None))
    }

    fn resolve(&self, lang: Lang, rel: &str, spec: &str, package: &Package) -> Option<String> {
        match lang {
            Lang::Rust => {
                let spec = spec.trim_start_matches("::");
                let mut parts = spec.split("::").map(|s| s.trim_matches(['{', '}', ' ']));
                let first = parts.next()?;
                let second = parts.next().filter(|s| !s.is_empty() && *s != "*");
                if first == "crate" {
                    return self.pick(package, second);
                }
                let krate = self.crates.get(first)?;
                self.pick(krate, second)
            }
            Lang::Swift => {
                let target = self.swift_targets.get(spec)?;
                self.pick(target, Some(spec)).or_else(|| {
                    // A target split into folders: its root files' module.
                    let prefix = format!("{}::{spec}/", module_id(target, None));
                    self.ids.iter().find(|id| id.starts_with(&prefix)).cloned()
                })
            }
            Lang::Python => {
                if let Some(relative) = spec.strip_prefix('.') {
                    // `from .sub import x`: this package's sub-package.
                    let name = relative.trim_start_matches('.').split('.').next();
                    return self.pick(package, name.filter(|n| !n.is_empty()));
                }
                let mut parts = spec.split('.');
                let top = self.python.get(parts.next()?)?;
                self.pick(top, parts.next())
            }
            Lang::TypeScript | Lang::Tsx => {
                if spec.starts_with('.') {
                    let target = normalize(&join(parent(rel), spec))?;
                    let candidates = [
                        target.clone(),
                        format!("{target}.ts"),
                        format!("{target}.tsx"),
                        format!("{target}.js"),
                        format!("{target}/index.ts"),
                        format!("{target}/index.tsx"),
                        format!("{target}/index.js"),
                    ];
                    let hit = candidates
                        .iter()
                        .find(|c| self.files.contains(c.as_str()))?;
                    let packages: Vec<Package> = self
                        .node
                        .values()
                        .chain(self.crates.values())
                        .chain(self.python.values())
                        .map(|p| (*p).clone())
                        .collect();
                    let (pkg, name) = place(hit, &packages);
                    return self.pick(&pkg, name.as_deref());
                }
                // `@scope/name/sub` or `name/sub`.
                let mut parts = spec.split('/');
                let first = parts.next()?;
                let name = if first.starts_with('@') {
                    format!("{first}/{}", parts.next()?)
                } else {
                    first.to_string()
                };
                let pkg = self.node.get(&name)?;
                self.pick(pkg, None)
            }
            Lang::Other => None,
        }
    }
}

/// `a/b/../c` → `a/c`; `None` past the root.
fn normalize(path: &str) -> Option<String> {
    let mut out: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            p => out.push(p),
        }
    }
    Some(out.join("/"))
}

#[cfg(test)]
pub(super) fn fold_for_test(modules: &mut Vec<Module>, omitted: &mut usize) {
    fold(modules, omitted, &mut HashMap::new())
}
