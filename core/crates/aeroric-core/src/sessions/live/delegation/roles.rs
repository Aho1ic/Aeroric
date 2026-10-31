//! The role library: four built-in roles, and the user's own (and overrides
//! of the built-ins) in `<dir>/roles.json` (Q-B15).

use std::path::Path;

use aeroric_protocol::checkpoint::Acceptance;
use aeroric_protocol::delegation::{
    permission_rank, AgentChoice, Budget, ContextMode, Role, Workspace,
};

const FILE: &str = "roles.json";

pub fn builtin() -> Vec<Role> {
    let role = |id: &str,
                name: &str,
                description: &str,
                instruction: &str,
                agent: AgentChoice,
                context_mode: ContextMode,
                permission: &str,
                workspace: Workspace| Role {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        instruction: instruction.into(),
        agent,
        context_mode,
        permission: permission.into(),
        workspace,
        acceptance: Vec::new(),
        budget: Budget::default(),
        builtin: true,
        customized: false,
    };
    vec![
        role(
            "researcher",
            "调研员",
            "只读调研：查代码、查资料，带回结论和依据",
            "只调研，不要修改任何文件。回答里写清结论、依据（文件路径、行号、命令输出）和仍不确定的地方。",
            AgentChoice::Same,
            ContextMode::Isolated,
            "plan",
            Workspace::ReadOnly,
        ),
        role(
            "reviewer",
            "审查员",
            "另一族只读审查：逐条列出问题",
            "只审查，不要修改任何文件。逐条列出问题，每条写明严重程度（阻断 / 重要 / 次要）、位置和理由。",
            AgentChoice::Other,
            ContextMode::Isolated,
            "plan",
            Workspace::ReadOnly,
        ),
        role(
            "implementer",
            "实现者",
            "带着交接清单改代码，自己验证",
            "按指令修改代码，改完自己运行能证明改对了的命令。回答里写清改了哪些文件、怎么验证的、还有什么没做。",
            AgentChoice::Same,
            ContextMode::Brief,
            "auto_edit",
            Workspace::Shared,
        ),
        role(
            "tester",
            "测试员",
            "补测试并运行",
            "为指令里的改动补测试并运行。回答里写清覆盖了哪些情况、运行结果，以及失败的输出。",
            AgentChoice::Same,
            ContextMode::Brief,
            "auto_edit",
            Workspace::Shared,
        ),
    ]
}

fn saved(dir: Option<&Path>) -> Vec<Role> {
    let Some(dir) = dir else {
        return Vec::new();
    };
    std::fs::read(dir.join(FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write(dir: &Path, roles: &[Role]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!(".{FILE}.tmp"));
    let bytes = serde_json::to_vec_pretty(roles).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join(FILE)).map_err(|e| e.to_string())
}

/// Built-ins (overridden where the user saved one of the same id), then the
/// user's own.
pub fn list(dir: Option<&Path>) -> Vec<Role> {
    let saved = saved(dir);
    let mut out: Vec<Role> = builtin()
        .into_iter()
        .map(|b| match saved.iter().find(|s| s.id == b.id) {
            Some(s) => Role {
                builtin: true,
                customized: true,
                ..s.clone()
            },
            None => b,
        })
        .collect();
    let builtin_ids: Vec<String> = out.iter().map(|r| r.id.clone()).collect();
    out.extend(
        saved
            .into_iter()
            .filter(|s| !builtin_ids.contains(&s.id))
            .map(|s| Role {
                builtin: false,
                customized: false,
                ..s
            }),
    );
    out
}

/// By id, else by display name.
pub fn find(dir: Option<&Path>, key: &str) -> Option<Role> {
    let key = key.trim().trim_start_matches('#');
    let all = list(dir);
    all.iter()
        .find(|r| r.id == key)
        .or_else(|| all.iter().find(|r| r.name == key))
        .cloned()
}

/// Save a role (a new id when it has none); the saved role.
pub fn save(dir: &Path, mut role: Role) -> Result<Role, String> {
    role.name = role.name.trim().to_string();
    if role.name.is_empty() {
        return Err("角色名不能为空".into());
    }
    if permission_rank(&role.permission).is_none() {
        return Err(format!("没有这个权限档：{}", role.permission));
    }
    if role.workspace == Workspace::ReadOnly && role.permission != "plan" {
        return Err("只读工作区的角色权限只能是只读（plan）".into());
    }
    role.acceptance
        .retain(|a: &Acceptance| !a.text.trim().is_empty());
    let mut all = saved(Some(dir));
    let builtin_ids: Vec<String> = builtin().into_iter().map(|r| r.id).collect();
    if role.id.trim().is_empty() {
        role.id = format!("role-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    }
    if list(Some(dir))
        .iter()
        .any(|r| r.name == role.name && r.id != role.id)
    {
        return Err(format!("已经有叫“{}”的角色", role.name));
    }
    let is_builtin = builtin_ids.contains(&role.id);
    role.builtin = false;
    role.customized = false;
    all.retain(|r| r.id != role.id);
    all.push(role.clone());
    write(dir, &all)?;
    Ok(Role {
        builtin: is_builtin,
        customized: is_builtin,
        ..role
    })
}

/// Delete the user's role, or restore a built-in to its default.
pub fn delete(dir: &Path, id: &str) -> Result<(), String> {
    let mut all = saved(Some(dir));
    let before = all.len();
    all.retain(|r| r.id != id);
    if all.len() == before {
        return Err(if builtin().iter().any(|r| r.id == id) {
            "内置角色不能删除，只能改".into()
        } else {
            "没有这个角色".into()
        });
    }
    write(dir, &all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_roles_override_builtins_and_deleting_restores_them() {
        let tmp =
            std::env::temp_dir().join(format!("aeroric-roles-{}", uuid::Uuid::new_v4().simple()));
        let dir = tmp.as_path();
        assert_eq!(list(Some(dir)).len(), 4);
        let mut researcher = find(Some(dir), "调研员").unwrap();
        researcher.instruction = "只看 docs/".into();
        let saved = save(dir, researcher).unwrap();
        assert!(saved.builtin && saved.customized);
        assert_eq!(
            find(Some(dir), "researcher").unwrap().instruction,
            "只看 docs/"
        );
        let mine = save(
            dir,
            Role {
                name: "文档员".into(),
                permission: "auto_edit".into(),
                workspace: Workspace::Shared,
                ..Role::default()
            },
        )
        .unwrap();
        assert!(mine.id.starts_with("role-") && !mine.builtin);
        assert_eq!(list(Some(dir)).len(), 5);
        assert!(save(
            dir,
            Role {
                name: "文档员".into(),
                permission: "plan".into(),
                ..Role::default()
            }
        )
        .unwrap_err()
        .contains("已经有"));
        assert!(save(
            dir,
            Role {
                name: "写只读".into(),
                permission: "auto_edit".into(),
                workspace: Workspace::ReadOnly,
                ..Role::default()
            }
        )
        .is_err());
        delete(dir, "researcher").unwrap();
        assert!(!find(Some(dir), "researcher").unwrap().customized);
        assert!(delete(dir, "researcher").unwrap_err().contains("内置"));
        delete(dir, &mine.id).unwrap();
        assert_eq!(list(Some(dir)).len(), 4);
    }
}
