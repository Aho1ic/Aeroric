# docs/ 规则

`docs/` 只写当前系统的事实；决策与取舍写进 `.agents/notes/`。入口是 [architecture.md](architecture.md)。

- 分层：全景 `architecture.md`；子系统 `subsystems/`；操作手册 `cookbook/`；事故复盘 `postmortem/`；产品候选库 `backlog.md`；门禁读取的机读工件 `native/`。不混写。
- 不写"以前/现在/不再/旧版"等变迁叙事，不手抄 API 列表或方法清单（以代码与 `contracts/` 为准）。
- 每篇不超过 300 行（`docs/native/` 的机读工件与 `backlog.md` 除外），`pnpm verify-docs` 检查长度、变迁词与相对链接。
- **`backlog.md` 绝不删除或覆盖**；改它只做局部编辑。
- 删除任何文档前先确认已入库（`git ls-files --error-unmatch <path>`）。
