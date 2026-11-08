use super::*;

#[test]
fn deleted_and_skipped_tests_are_flagged_with_where() {
    let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -10,8 +10,5 @@ mod tests {
     use super::*;
-    #[test]
-    fn adds() {
-        assert_eq!(add(1, 1), 2);
-    }
+    #[ignore]
     #[test]
     fn subtracts() {}
diff --git a/tests/old_test.py b/tests/old_test.py
deleted file mode 100644
--- a/tests/old_test.py
+++ /dev/null
@@ -1,2 +0,0 @@
-def test_old():
-    pass
";
    assert_eq!(
        flags(diff),
        vec![
            "src/lib.rs:11 加了 `#[ignore`，跳过了测试".to_string(),
            "src/lib.rs:11 删掉了 1 个测试".to_string(),
            "tests/old_test.py 整个测试文件被删了".to_string(),
        ]
    );
}

#[test]
fn a_moved_test_and_ordinary_code_are_not_flagged() {
    let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,3 +1,3 @@
-#[test]
-fn old_name() {}
+#[test]
+fn new_name() {}
 let skip = 1;
";
    assert!(flags(diff).is_empty());
}

#[test]
fn silenced_checks_and_loosened_ci_are_flagged() {
    let diff = "\
diff --git a/web/app.ts b/web/app.ts
--- a/web/app.ts
+++ b/web/app.ts
@@ -5,0 +5,1 @@
+// @ts-ignore
diff --git a/.github/workflows/ci.yml b/.github/workflows/ci.yml
--- a/.github/workflows/ci.yml
+++ b/.github/workflows/ci.yml
@@ -20,3 +20,3 @@
-      - run: cargo test --workspace
-      - run: cargo clippy -- -D warnings
+      - run: cargo clippy -- -D warnings
+        continue-on-error: true
";
    assert_eq!(
        flags(diff),
        vec![
            "web/app.ts:5 加了 `@ts-ignore`，压掉了检查".to_string(),
            ".github/workflows/ci.yml:21 加了 `continue-on-error: true`，检查失败也算过"
                .to_string(),
            ".github/workflows/ci.yml 删掉了检查 `- run: cargo test --workspace`".to_string(),
        ]
    );
}
