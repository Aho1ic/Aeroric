//! 守卫:`#[tauri::command]` 的定义集合必须与 `lib.rs` 里 `generate_handler!` 的
//! 注册集合完全相等。
//!
//! 为什么需要它:漏注册是**运行时**失败——`cargo build` 和 `cargo clippy` 都不会报错,
//! 前端调用时才会拿到 "command not found"。命令数量已经到 557 个,靠 review diff
//! 盯住"新增 command 有没有同步加到 generate_handler!"不可靠。
//!
//! 这个测试读源码而不是读符号表,因为 `generate_handler!` 展开后不保留可枚举的清单。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// 源码根目录。`CARGO_MANIFEST_DIR` 在编译期定死,不受测试进程 cwd 影响。
fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// 命令名 -> 定义位置(`文件:行号`),失败信息里直接能点开。
fn defined_commands() -> BTreeMap<String, String> {
    let root = source_root();
    let mut files = Vec::new();
    rust_sources(&root, &mut files);
    files.sort();

    let mut defined = BTreeMap::new();
    for file in files {
        let text = fs::read_to_string(&file).unwrap_or_else(|e| panic!("read {file:?}: {e}"));
        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if !line.trim_start().starts_with("#[tauri::command") {
                continue;
            }
            // 属性和 fn 之间可能还夹着别的属性,往下找最近的 fn 签名。
            let name = lines[index + 1..]
                .iter()
                .take(8)
                .filter(|candidate| !candidate.trim_start().starts_with("//"))
                .find_map(|candidate| function_name(candidate));
            let name = name.unwrap_or_else(|| {
                panic!(
                    "{}:{} 的 #[tauri::command] 后面 8 行内找不到 fn 签名",
                    file.display(),
                    index + 1
                )
            });
            let relative = file.strip_prefix(&root).unwrap_or(&file).display();
            let location = format!("src/{relative}:{}", index + 1);
            if let Some(previous) = defined.insert(name.clone(), location.clone()) {
                // 同名命令会让"按名字比集合"这件事失去意义(一个漏注册会被另一个
                // 掩盖),而且 Tauri 侧本身也不允许重名。
                panic!("命令 `{name}` 被定义了两次:{previous} 与 {location}");
            }
        }
    }
    defined
}

fn function_name(line: &str) -> Option<String> {
    let after_fn = line.split_once("fn ")?.1;
    let name: String = after_fn
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// 解析 `lib.rs` 里 `tauri::generate_handler![...]` 的条目,取每条的最后一段路径。
fn registered_commands() -> BTreeSet<String> {
    let text = fs::read_to_string(source_root().join("lib.rs")).expect("read lib.rs");
    let macro_start = text
        .find("tauri::generate_handler![")
        .expect("lib.rs 里找不到 tauri::generate_handler!");
    let open = text[macro_start..].find('[').expect("找不到 `[`") + macro_start;

    let mut depth = 0usize;
    let mut close = None;
    for (offset, ch) in text[open..].char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close.expect("generate_handler! 的方括号没有闭合");

    let mut registered = BTreeSet::new();
    for raw in text[open + 1..close].split(',') {
        // 行内注释不是条目。
        let entry = raw
            .lines()
            .map(|line| line.split("//").next().unwrap_or_default().trim())
            .find(|line| !line.is_empty());
        let Some(entry) = entry else { continue };
        let name = entry
            .rsplit("::")
            .next()
            .unwrap_or(entry)
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        assert!(
            registered.insert(name.clone()),
            "`{name}` 在 generate_handler! 里注册了两次"
        );
    }
    registered
}

#[test]
fn every_tauri_command_is_registered_in_generate_handler() {
    let defined = defined_commands();
    let registered = registered_commands();

    // 先确认两侧解析器都真的解析到了东西。解析器静默返回空集合会让下面的相等断言
    // 变成永真——这正是本测试要防的那类"绿灯不是绿灯"。
    assert!(
        defined.len() > 400,
        "只解析到 {} 个 #[tauri::command],解析器大概率坏了",
        defined.len()
    );
    assert!(
        registered.len() > 400,
        "只解析到 {} 条注册,generate_handler! 的解析大概率坏了",
        registered.len()
    );

    let defined_names: BTreeSet<String> = defined.keys().cloned().collect();

    let missing: Vec<String> = defined_names
        .difference(&registered)
        .map(|name| format!("{name}({})", defined[name]))
        .collect();
    assert!(
        missing.is_empty(),
        "以下 command 定义了但没有在 lib.rs 的 generate_handler! 里注册,前端调用会\
         直接失败:\n  {}",
        missing.join("\n  ")
    );

    let unknown: Vec<&String> = registered.difference(&defined_names).collect();
    assert!(
        unknown.is_empty(),
        "generate_handler! 注册了不存在对应 #[tauri::command] 的名字:{unknown:?}"
    );
}
