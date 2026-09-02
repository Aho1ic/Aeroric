//! dsh 源码 checkout 的构建就绪判定,以及启动失败的成因翻译。
//!
//! 从 `dsh_webui.rs` 拆出来:一个 checkout 声明了哪些构建产物、哪些还没生成、
//! 以及一段启动失败的输出是"构建不全"还是别的原因。
//!
//! 这块只读文件系统和字符串,不碰 `AppHandle` / `DshWebUiManager` —— 输入是
//! `&Path` 与 `&str`,输出是 `Vec<PathBuf>` 与 `String`。所以判定逻辑可以在
//! 临时目录上直接测,不必起一个 dsh 进程。

use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 报错里最多列几个缺失产物。全列出来会有几十条,反而埋掉修复指引。
const DSH_MISSING_ARTIFACT_SAMPLE: usize = 5;

/// 一个包声明的、指向 `lib/` 的构建产物。只收 `.js`:`.d.ts` 缺失不影响运行。
pub(super) fn declared_lib_artifacts(package_dir: &Path) -> Vec<PathBuf> {
    let Ok(raw) = std::fs::read_to_string(package_dir.join("package.json")) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    // 只看 DSH 自己的包。node_modules 里的第三方包不该被我们当作构建目标。
    if !value
        .get("name")
        .and_then(Value::as_str)
        .is_some_and(|name| name.starts_with("@deepseek-ai/"))
    {
        return Vec::new();
    }

    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |relative: &str| {
        let trimmed = relative.trim_start_matches("./");
        // 只认 lib/ 下的 js 产物;`./src/*` 这类通配导出不是构建输出。
        if trimmed.starts_with("lib/") && trimmed.ends_with(".js") && !trimmed.contains('*') {
            // `main` 与 `exports["."]` 常指向同一个文件,去重,否则同一个缺失
            // 产物会在错误里报两遍、把计数也撑大。
            if seen.insert(trimmed.to_string()) {
                targets.push(package_dir.join(trimmed));
            }
        }
    };

    if let Some(main) = value.get("main").and_then(Value::as_str) {
        push(main);
    }
    if let Some(exports) = value.get("exports").and_then(Value::as_object) {
        for entry in exports.values() {
            match entry {
                Value::String(path) => push(path),
                // 条件导出:只取 `default`,不追 `types`(那是 .d.ts)。
                Value::Object(conditions) => {
                    if let Some(path) = conditions.get("default").and_then(Value::as_str) {
                        push(path);
                    }
                }
                _ => {}
            }
        }
    }
    targets
}

/// checkout 里所有声明了 `lib/` 产物的包目录。
///
/// 不硬编码具体包名:上游新增包会自动纳入,删包也不会留下失效断言。
pub(super) fn dsh_package_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    // `packages/<group>/<pkg>` 两层,`apps/<pkg>` 一层。
    let mut push_children = |parent: PathBuf, recurse: bool| {
        let Ok(entries) = std::fs::read_dir(&parent) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if path.join("package.json").is_file() {
                dirs.push(path.clone());
            }
            if recurse {
                if let Ok(children) = std::fs::read_dir(&path) {
                    for child in children.flatten() {
                        let child = child.path();
                        if child.is_dir() && child.join("package.json").is_file() {
                            dirs.push(child);
                        }
                    }
                }
            }
        }
    };
    push_children(root.join("packages"), true);
    push_children(root.join("apps"), false);
    dirs
}

/// checkout 是否具备启动 `dsh web` 所需的构建产物。
///
/// 之前这里只查 `apps/cli/lib/bin.js`,但 checkout 的启动命令是
/// `pnpm --dir <root> dsh`,而该 script 是 `node --import tsx/esm apps/cli/src/bin.ts`
/// —— 走的是 **src**,`lib/bin.js` 根本不参与启动。真正需要的是各包 `exports`/`main`
/// 指向的 `lib/*.js`:plugin tree 与 typert loader 在运行时按这些导出解析。
/// 于是"旧构建 + git pull 后新增包未构建"这种部分构建状态能顺利过闸,
/// 直到 30 秒后报一句 "did not report its startup URL"。
///
/// 返回实际缺失的产物路径,便于把结论直接写进报错。
pub(super) fn dsh_checkout_missing_artifacts(root: &Path) -> Vec<PathBuf> {
    dsh_package_dirs(root)
        .into_iter()
        .flat_map(|dir| declared_lib_artifacts(&dir))
        .filter(|artifact| !artifact.is_file())
        .collect()
}

/// 把缺失产物变成一句能照着做的错误。
pub(super) fn checkout_not_built_error(root: &Path, missing: &[PathBuf]) -> String {
    let mut message = format!(
        "DeepSeek Harness source at {} is not fully built: {} build artifact(s) are missing.",
        root.display(),
        missing.len()
    );
    for artifact in missing.iter().take(DSH_MISSING_ARTIFACT_SAMPLE) {
        let shown = artifact
            .strip_prefix(root)
            .unwrap_or(artifact)
            .to_string_lossy()
            .replace('\\', "/");
        message.push_str(&format!("\n  - {shown}"));
    }
    if missing.len() > DSH_MISSING_ARTIFACT_SAMPLE {
        message.push_str(&format!(
            "\n  ... and {} more",
            missing.len() - DSH_MISSING_ARTIFACT_SAMPLE
        ));
    }
    message.push_str(&format!(
        "\n\nRun `pnpm install` and `pnpm run build` in {}, then retry. \
         A stale build after `git pull` looks exactly like this. \
         Alternatively, point the DSH executable at an Aeroric-managed install, \
         which ships prebuilt bundles and needs neither pnpm nor a build step.",
        root.display()
    ));
    message
}

/// DSH 启动输出里"构建不完整"的特征串。
///
/// 就绪闸门与这一层互为冗余:闸门按声明的 `exports` 判断,这一层按进程实际的
/// 抱怨判断。上游改了产物布局导致闸门看不出问题时,这里仍能给出正确结论;
/// 反之闸门能在启动前就拦下,不必等进程跑起来。
pub(super) const DSH_BUILD_FAILURE_SIGNATURES: &[&str] = &[
    "plugin tree failed to load",
    "client bundles not found",
    "run `pnpm run build` before launch",
    "failed to compose",
];

/// 输出是否表明 checkout 没构建完整。
pub(super) fn looks_like_incomplete_build(output: &str) -> bool {
    if DSH_BUILD_FAILURE_SIGNATURES
        .iter()
        .any(|signature| output.contains(signature))
    {
        return true;
    }
    // `Cannot find module .../lib/xxx.js` 单独成立:缺的是构建产物,
    // 而不是缺依赖(那会指向 node_modules)。
    output.contains("Cannot find module")
        && output.contains("/lib/")
        && !output.contains("/node_modules/")
}

/// 给启动失败补上结论。
///
/// 裸抛 "did not report its startup URL" + 16KB 堆栈时,真正的原因
/// (少了几个 lib 产物)埋在第 20 行开外,用户无从下手。
pub(super) fn explain_dsh_web_failure(error: String, launch_root: Option<&Path>) -> String {
    if !looks_like_incomplete_build(&error) {
        return error;
    }
    let remedy = match launch_root {
        Some(root) => format!(
            "The DeepSeek Harness source checkout at {} is not fully built. \
             Run `pnpm install` and `pnpm run build` there, then retry. \
             A stale build after `git pull` looks exactly like this.",
            root.display()
        ),
        None => "The active DeepSeek Harness installation is missing build artifacts. \
             Reinstall it, or switch to an Aeroric-managed install, which ships prebuilt bundles."
            .to_string(),
    };
    format!("{remedy}\n\n{error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个最小 checkout:一个包 + 声明的 lib 产物。
    /// `built` 决定产物文件是否真的落盘。
    fn fake_dsh_checkout(built: bool) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("aeroric-dsh-readiness-{}", uuid::Uuid::new_v4()));
        let package = root
            .join("packages")
            .join("context")
            .join("session-reference");
        std::fs::create_dir_all(package.join("lib")).expect("the package tree is created");
        std::fs::write(
            package.join("package.json"),
            r#"{
              "name": "@deepseek-ai/dsh-session-reference",
              "main": "lib/index.js",
              "exports": {
                ".": { "types": "./lib/types/index.d.ts", "default": "./lib/index.js" },
                "./typert": { "default": "./lib/typert.host.js" },
                "./src/*": "./src/*",
                "./package.json": "./package.json"
              }
            }"#,
        )
        .expect("the manifest is written");
        if built {
            for name in ["index.js", "typert.host.js"] {
                std::fs::write(package.join("lib").join(name), "export {};\n")
                    .expect("the artifact is written");
            }
        }
        root
    }

    /// 已构建的 checkout 必须放行。闸门若在这里误报,会把本来能用的机器彻底挡死。
    #[test]
    fn a_fully_built_checkout_passes_the_readiness_gate() {
        let root = fake_dsh_checkout(true);
        assert!(dsh_checkout_missing_artifacts(&root).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    /// 本次 bug 的回归:`lib/typert.host.js` 缺失时必须在启动前就被认出来,
    /// 而不是等 `dsh web` 跑起来再抛 "did not report its startup URL"。
    #[test]
    fn a_missing_typert_host_bundle_is_reported_with_its_path() {
        let root = fake_dsh_checkout(false);
        let missing = dsh_checkout_missing_artifacts(&root);
        assert_eq!(missing.len(), 2, "both declared lib artifacts are missing");
        let error = checkout_not_built_error(&root, &missing);
        assert!(error.contains("typert.host.js"), "got: {error}");
        assert!(error.contains("pnpm run build"), "got: {error}");
        let _ = std::fs::remove_dir_all(root);
    }

    /// `./src/*` 这类通配导出与 `types` 指向的 .d.ts 都不是构建产物,
    /// 不能让它们把一个健康的 checkout 判成未构建。
    #[test]
    fn wildcard_exports_and_type_declarations_are_not_treated_as_build_output() {
        let root = fake_dsh_checkout(true);
        let package = root
            .join("packages")
            .join("context")
            .join("session-reference");
        let declared = declared_lib_artifacts(&package);
        assert!(declared
            .iter()
            .all(|path| path.extension().is_some_and(|e| e == "js")));
        assert!(!declared
            .iter()
            .any(|path| path.to_string_lossy().contains('*')));
        assert!(!declared
            .iter()
            .any(|path| path.to_string_lossy().ends_with(".d.ts")));
        let _ = std::fs::remove_dir_all(root);
    }

    /// 第三方包不该被当成我们的构建目标,否则 vendor 目录会制造大量假缺失。
    #[test]
    fn packages_outside_the_deepseek_scope_declare_no_artifacts() {
        let root =
            std::env::temp_dir().join(format!("aeroric-dsh-thirdparty-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("the directory is created");
        std::fs::write(
            root.join("package.json"),
            r#"{ "name": "lodash", "main": "lib/index.js" }"#,
        )
        .expect("the manifest is written");
        assert!(declared_lib_artifacts(&root).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    /// 闸门放过时,进程自己的抱怨要能被翻译成结论。这是与闸门互为冗余的一层。
    #[test]
    fn build_failure_signatures_are_translated_into_a_conclusion() {
        for output in [
            "Error: dsh: plugin tree failed to load: loader fibers failed",
            "client bundles not found; run `pnpm run build` before launch:",
            "client-modules: 4 client packages failed to compose:",
            "Cannot find module '/src/dsh/packages/context/session-reference/lib/typert.host.js'",
        ] {
            assert!(looks_like_incomplete_build(output), "missed: {output}");
            let explained =
                explain_dsh_web_failure(output.to_string(), Some(Path::new("/src/dsh")));
            assert!(explained.contains("not fully built"), "got: {explained}");
            assert!(explained.contains("pnpm run build"), "got: {explained}");
            // 原始输出必须保留在后面,诊断只是前置结论,不能吞掉证据。
            assert!(explained.contains(output), "the raw output survives");
        }
    }

    /// 缺依赖(node_modules)不是构建问题,别给出"去 build"的错误指引。
    #[test]
    fn a_missing_dependency_is_not_reported_as_an_incomplete_build() {
        let output = "Cannot find module '/src/dsh/node_modules/commander/index.js'";
        assert!(!looks_like_incomplete_build(output));
        assert_eq!(
            explain_dsh_web_failure(output.to_string(), Some(Path::new("/src/dsh"))),
            output
        );
    }

    /// 无关失败(端口占用之类)必须原样透传,不能被套上构建结论。
    #[test]
    fn unrelated_startup_failures_pass_through_untouched() {
        let output = "DSH Web exited before becoming ready (exit status: 1)".to_string();
        assert_eq!(
            explain_dsh_web_failure(output.clone(), Some(Path::new("/src/dsh"))),
            output
        );
    }
}
