//! P7-02 Action 1：unix 分支（cfg 隔离；不删除 Windows 分支）。
/// Unix 平台的路径探测（usr/local/bin、homebrew 等）。
pub fn candidate_paths(binary: &str) -> Vec<String> {
    vec![
        format!("/usr/local/bin/{binary}"),
        format!("/opt/homebrew/bin/{binary}"),
        format!("/usr/bin/{binary}"),
    ]
}

#[cfg(test)]
mod tests {
    #[test]
    fn candidate_paths_cover_common_locations() {
        let paths = super::candidate_paths("git");
        assert!(paths.contains(&"/usr/local/bin/git".to_string()));
        assert!(paths.contains(&"/opt/homebrew/bin/git".to_string()));
    }
}
