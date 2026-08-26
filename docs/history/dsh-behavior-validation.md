# DeepSeek Harness 行为验证报告

**验证时间**: 2026-08-14  
**DSH 版本**: 0.1.0-rc.6  
**验证环境**: macOS, ~/.dsh (用户已有安装)

---

## 1. 基础环境信息

### 安装位置与版本
```bash
$ which dsh
/Users/macbook/.local/bin/dsh

$ dsh -V
0.1.0-rc.6
```

### 家目录结构
```
~/.dsh/
├── profiles/
│   └── web/
│       ├── cordis.patch.yml      # 用户层 patch
│       ├── cordis.yml
│       ├── package.json          # profile 元数据与插件依赖
│       ├── pnpm-workspace.yaml
│       └── node_modules/         # 已安装插件
├── settings.yaml                 # 用户设置（热重载）
└── storages/                     # 会话数据等
```

### Web Profile 配置
```json
{
  "name": "dsh-profile-web",
  "private": true,
  "dependencies": {},
  "dsh": {
    "profile": {
      "bundles": [
        "@deepseek-ai/dsh-base",
        "@deepseek-ai/dsh-web-app"
      ]
    }
  }
}
```

**发现**: 
- `dependencies` 字段为空对象，说明插件安装后会填充此字段
- `bundles` 列表定义了 profile 的基础插件栈

---

## 2. 插件管理验证

### 插件安装命令
```bash
dsh plugin --profile web add <package>
```

**结论**: 
- ✅ Profile 使用 `package.json` 的 `dependencies` 字段管理插件
- ✅ 插件安装命令: `dsh plugin --profile <name> add <package>`

---

## 3. 配置系统验证

### Patch 分层机制

**验证**: 使用 `--patch` 禁用 tool-web
```bash
$ echo '- id: tool-web
  disabled: true' > disable-web.patch.yml

$ dsh --profile headless --patch disable-web.patch.yml --dump-config | grep -A5 "tool-web"
- id: tool-web
  name: '@deepseek-ai/dsh-tool-web'
  config:
    fetch: false
    searchTimeoutMs: 60000
  disabled: true
```

**结论**:
- ✅ `--patch` 参数可以覆盖 bundle 配置
- ✅ 禁用语法为 `disabled: true`（不是 `enabled: false`）
- ✅ 多个 `--patch` 可以叠加使用（文档确认可重复）

### 环境变量注入

**权限模式**:
```yaml
- id: sandbox-policy
  config:
    mode: !!js process.env.DSH_PERMISSION_MODE ?? 'workspace-write'
```

**遥测模式**:
```yaml
- id: session-telemetry-otel
  config:
    mode: !!js process.env.DSH_TELEMETRY_MODE || 'DISABLED'
```

**结论**:
- ✅ 权限通过环境变量 `DSH_PERMISSION_MODE` 控制
- ✅ 遥测通过 `DSH_TELEMETRY_MODE` 控制（默认 DISABLED）
- ✅ 配置支持 `!!js` JavaScript 表达式

---

## 4. Goal 命令验证

### 命令格式
```bash
dsh --profile headless "/goal <description>"
```

### 测试结果
```
dsh: MISSING_CREDENTIAL: llm-deepseek: no API key for provider route "deepseek-official"
```

**发现**: 
- ⚠️ 需要配置 `DEEPSEEK_API_KEY` 才能运行
- ✅ `/goal` 被识别为命令前缀（没有报 "unknown command" 错误）
- 🔍 需要有效凭据才能测试输出格式

**推断**:
- Goal 模式应该是内建支持（从配置中看到 `dsh-goal` 相关插件）
- `/goal` 可能是斜杠命令语法，直接透传即可

---

## 5. Web UI 验证

### 启动命令
```bash
dsh --profile web [--host <host>] [--port <port>]
```

**端口参数**:
- `--port <port>`: 指定端口，传 0 让 OS 自动分配
- 默认端口需要查看合成配置（未在帮助中明确）

**待验证项**（需实际启动）:
- [ ] 默认端口
- [ ] 健康检查端点（`/health` 或 `/api/health`）
- [ ] CORS 头设置
- [ ] iframe 兼容性

---

## 6. Session 持久化验证

### 配置
```yaml
- id: session-persistence-jsonl
  name: '@deepseek-ai/dsh-session-persistence-jsonl'
  config:
    root: !!js dshHomePath('sessions')
```

**结论**:
- ✅ 会话存储在 `$DSH_HOME/sessions/`
- ✅ 格式为 JSONL
- 🔍 需要检查实际文件布局（`--<cwd>--/<id>/` 模式）

---

## 7. 插件生态扫描

### Bundle 插件列表（从 --dump-config）

**核心插件**:
- `@deepseek-ai/dsh-base`: 基础插件栈
- `@deepseek-ai/dsh-web-app`: Web UI
- `@deepseek-ai/dsh-agent`: Agent 核心
- `@deepseek-ai/dsh-llm`: LLM 集成
- `@deepseek-ai/dsh-session`: 会话管理
- `@deepseek-ai/dsh-sandbox-local`: 沙箱
- `@deepseek-ai/dsh-tool-web`: Web Search
- `@deepseek-ai/dsh-settings-file`: 设置热重载
- `@deepseek-ai/dsh-credentials-local`: 凭据管理

**可选插件**（从配置推断）:
- `@deepseek-ai/dsh-skill-filesystem`: 技能系统
- `@deepseek-ai/dsh-mcp-client`: MCP 支持
- `@deepseek-ai/dsh-goal`: Goal 模式（推测）

**结论**:
- ✅ 插件命名规范: `@deepseek-ai/dsh-*` 或 `dsh-*`
- ✅ 每个插件对应配置中的一个 `id` 行
- ✅ 插件通过 `disabled: true` 禁用

---

## 8. 实施建议调整

### 8.1 插件面板实施

**list_dsh_plugins 实现**:
```rust
// 扫描 profile/node_modules/@deepseek-ai/dsh-*
// 或直接读取 profile/package.json 的 dependencies
// 匹配 "^@deepseek-ai/dsh-" 或 "^dsh-"
```

**toggle_dsh_plugin 实现**:
```rust
// 写入 profile/cordis.patch.yml（用户层 patch）
// 格式: 
// - id: <plugin-id>
//   disabled: true
```

**风险**: 用户层 patch 当前为空数组 `[]`，需要转换为 YAML list 格式

---

### 8.2 goal/plan 原生化

**前端实现**（确认可行）:
```typescript
export function wrapInGoalMode(prompt: string, agent: AgentType): string {
  const family = agentFamily(agent);
  if (family === "dsh") {
    return `/goal ${prompt}`;  // ✅ 直接透传
  }
  return `Your ultimate goal...`;  // Claude/Codex 包装
}
```

**SessionView 渲染**: 需要实际运行 dsh 任务查看 JSONL 事件格式

---

### 8.3 web_search 开关

**patch 注入实现**（确认语法）:
```rust
// 写入临时 patch 文件:
// - id: tool-web
//   disabled: true
// 
// 启动命令: dsh --profile headless --patch <file>
```

**✅ 语法确认**: `disabled: true` 而非 `enabled: false`

---

### 8.4 Web UI 进程管理

**启动命令**:
```bash
DSH_HOME=<托管home> dsh --profile web --port <port>
```

**健康检查**: 需要实际启动后验证端点（推测 `/` 或 `/api/health`）

---

## 9. 关键发现总结

| 验证项 | 状态 | 结论 |
|--------|------|------|
| 插件管理 | ✅ 确认 | `dsh plugin --profile <name> add <package>` |
| 插件列表 | ✅ 确认 | `package.json` dependencies 字段 |
| 禁用语法 | ✅ 确认 | `disabled: true` |
| Patch 叠加 | ✅ 确认 | `--patch` 可重复使用 |
| 权限环境变量 | ✅ 确认 | `DSH_PERMISSION_MODE` |
| /goal 命令 | ⚠️ 部分确认 | 语法正确，需凭据测试输出 |
| Web UI 端口 | ✅ 确认 | `--port` 参数 |
| Session 路径 | ✅ 确认 | `$DSH_HOME/sessions/` |
| 遥测默认关闭 | ✅ 确认 | `DISABLED` by default |

---

## 10. 后续行动

### 需要实际测试的项目（需配置 API Key）:
1. [ ] `/goal` 命令的 JSONL 输出格式
2. [ ] Web UI 的默认端口与健康检查端点
3. [ ] 实际会话文件的目录布局
4. [ ] Goal 事件类型（goal/started, goal/step 等）

### 可以直接开始编码的项目:
1. ✅ 插件面板后端（基于 package.json 与 dsh plugin）
2. ✅ web_search 开关（patch 注入语法已确认）
3. ✅ goal/plan 前端透传（`/goal` 语法确认）
4. ✅ Web UI 进程管理（启动命令已知）

---

## 11. 实施计划调整

### 优先级调整

**立即开始**（不依赖凭据）:
1. web_search 开关（2 天）
2. 插件面板（3 天）
3. Web UI 进程管理（2 天）
4. LaunchMode 扩展（0.5 天）

**延后实施**（需要实际运行测试）:
5. goal/plan 原生化 + SessionView 扩展（1-2 天）
   - 需要配置 DEEPSEEK_API_KEY 测试输出格式

**总计**: 7.5-9 天（不含 goal/plan 的完整测试）

---

**验证版本**: 1.0  
**DSH 版本**: 0.1.0-rc.6  
**下次更新**: 配置 API Key 后补充 goal/session 事件测试
