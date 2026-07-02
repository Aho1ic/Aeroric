# Shadcn b5xKlRYuO Style Refresh Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` when available. In the current Codex session, no callable `superpowers:*` or Trellis tool is exposed, so track work with this file and the active Codex plan. Steps use checkbox (`- [ ]`) syntax for progress tracking.

**Goal:** 在不修改前端框架的前提下，使用 shadcn/ui `b5xKlRYuO` preset 的视觉语言刷新 Aeroric 前端样式。保留现有 Vite + React + Tauri 架构、路由/入口、Tauri API 调用、组件业务逻辑和测试语义。

**Non-goals:**
- 不迁移到 Next.js。
- 不直接在仓库根目录执行 `pnpm dlx shadcn@latest init --preset b5xKlRYuO --template next`。
- 不重写 Rust/Tauri command、事件监听、数据持久化、终端/数据库/SSH/文件浏览器业务逻辑。
- 不做大规模组件重构；优先通过设计 token、通用控件样式和局部样式收敛达成 shadcn 风格。

**Preset facts:** `pnpm dlx shadcn@latest preset decode b5xKlRYuO` 输出为 `style=luma`、`baseColor=mist`、`theme=violet`、`chartColor=violet`、`iconLibrary=hugeicons`、`font=inter`、`radius=large`、`menuAccent=bold`、`menuColor=default`。

**Architecture:** 保留现有 `src/main.tsx`、`src/App.tsx`、`vite.config.ts`、`src-tauri/tauri.conf.json` 和所有 Tauri API 使用方式。把 shadcn `b5xKlRYuO` 作为样式规范来源，将核心视觉映射到现有 CSS variables 和少量共享 UI 控件中。现有 TS inline style 模块继续工作，优先改 `src/styles/themes.css` 的 token，再按风险局部调整 `src/App.css`、`src/styles/*.ts` 和 `src/components/ui/Button.tsx`。

**Tech Stack:** Vite 8、React 19、TypeScript、Tauri 2、pnpm、shadcn/ui preset `b5xKlRYuO`、hugeicons preset target、existing lucide-react app icons unless explicitly migrated。

---

## Trellis Board

### Backlog
- [ ] 验证 Tauri 桌面启动。

### Doing
- [ ] 浏览器截图受限于当前 Vite 环境的 Tauri metadata 运行时错误，需后续在 Tauri shell 中做完整视觉巡检。

### Done
- [x] 读取项目框架：当前为 Vite + React + Tauri。
- [x] 读取 shadcn skill 规则。
- [x] 解析 `b5xKlRYuO` preset。
- [x] 确认最新用户约束：不修改前端框架，只修改样式。
- [x] 在 `dev` 分支完成 shadcn `b5xKlRYuO` token 映射，不引入 Next 模板文件。
- [x] 迁移 light/dark/eyecare token。
- [x] 收敛通用控件、弹层、菜单、输入框、按钮视觉。
- [x] 验证 TypeScript、lint、测试和生产构建。

---

## Task 1: Baseline And Guardrails

**Files:**
- Read: `package.json`
- Read: `vite.config.ts`
- Read: `src-tauri/tauri.conf.json`
- Read: `src/main.tsx`
- Read: `src/App.tsx`
- Read: `src/App.css`
- Read: `src/styles/themes.css`
- Read: `src/styles/*.ts`

- [x] Record current git status and avoid touching unrelated user changes.
- [x] Run `pnpm dlx shadcn@latest preset decode b5xKlRYuO` only as read-only preset inspection.
- [x] Do not run `pnpm dlx shadcn@latest init --preset b5xKlRYuO --template next` in the repo.
- [x] If shadcn CLI output is needed for reference, generate it in a disposable temp directory only, then manually port style-relevant tokens.
- [ ] Capture baseline UI screenshots for at least welcome/project shell, project workspace, settings dialog, database view, and terminal view.

## Task 2: Token Mapping

**Files:**
- Modify: `src/styles/themes.css`
- Optional Modify: `src/App.css`

- [x] Map shadcn mist/violet/luma tokens into existing Aeroric variables instead of forcing Tailwind class migration.
- [x] Keep existing semantic variables stable: `--bg-root`, `--bg-panel`, `--bg-card`, `--bg-hover`, `--border-*`, `--text-*`, `--accent`, `--primary-action-*`, `--radius-*`, `--shadow-*`.
- [x] Refresh light theme toward shadcn mist/violet: cleaner mist-tinted neutral surfaces, violet accent states, consistent focus color, and bold menu accent behavior.
- [x] Refresh dark theme to align with luma/violet while keeping contrast readable for code, terminal, and data grids.
- [x] Keep `eyecare` as a distinct warm accessibility theme, but align radius, shadows, focus, and control states with the new system.
- [x] Add shadcn-compatible aliases only if useful for future components, for example `--background`, `--foreground`, `--card`, `--card-foreground`, `--primary`, `--primary-foreground`, without breaking current variables.
- [x] Keep app minimum width and Tauri window assumptions unchanged.

## Task 3: Shared Controls

**Files:**
- Modify: `src/components/ui/Button.tsx`
- Optional Modify: `src/App.css`
- Optional Modify: `src/styles/common.ts`

- [x] Align the local `Button` variants with shadcn semantics: `default`, `outline`, `secondary`, `ghost`, `destructive`, `link`.
- [x] Keep the existing Button API unchanged so call sites do not need behavior changes.
- [x] Tune heights, radius, border, focus ring, disabled state, active state, and hover state to match the new token system.
- [x] Ensure icon buttons keep stable dimensions and do not shift layout.
- [x] Review global Radix Select and branch popover CSS for shadcn-like menu density, radius, focus, and item hover.

## Task 4: Shell And High-Traffic Surfaces

**Files:**
- Modify: `src/styles/layout.ts`
- Modify: `src/styles/panels.ts`
- Modify: `src/styles/dialogs.ts`
- Modify: `src/styles/terminal.ts`
- Modify: `src/styles/database.ts`
- Modify: `src/styles/git-diff.ts`
- Modify: `src/styles/task.ts`
- Modify: `src/styles/skill-hub.ts`
- Modify: `src/styles/timeline.ts`

- [x] Update app shell surfaces: sidebar, rail, workspace background, title/top bars.
- [x] Update panels and dialogs so borders, shadows, radius, and padding feel consistent.
- [x] Keep dense operational layout; do not convert the app into a marketing/card-heavy layout.
- [x] Preserve terminal readability and xterm theme behavior.
- [x] Preserve database table density and scanning performance; only tune visual tokens, row hover, selected state, and toolbar polish.
- [x] Preserve git diff semantic colors for add/delete/hunk while aligning neutral containers.
- [x] Ensure all text remains within current fixed desktop constraints.

## Task 5: Optional Shadcn Artifacts

**Files:**
- Optional Create: `components.json`
- Optional Create/Modify: `src/lib/utils.ts` or existing utility path if needed
- Optional Add: selected `src/components/ui/*` files only if directly used

- [x] Prefer no new shadcn generated components unless there is a concrete style implementation need.
- [ ] If `components.json` is introduced, configure it for the existing Vite app and existing import aliases; do not introduce Next app files.
- [ ] If generated components are added, run `pnpm dlx shadcn@latest docs <component>` before using them and inspect the generated source.
- [x] Treat `hugeicons` as the preset target. Because this is style-only, keep existing `lucide-react` app icons unless a later implementation step explicitly migrates icon components and dependencies.
- [x] Avoid broad Tailwind migration unless explicitly approved after this style-only phase.

## Task 6: Verification Before Completion

**Commands:**
- Verify: `pnpm exec tsc --noEmit`
- Verify: `pnpm test`
- Verify: `pnpm lint`
- Verify: `pnpm build`
- Optional Verify: `pnpm tauri build --bundles app`

- [x] Run TypeScript verification.
- [x] Run focused tests first if style changes touch component behavior or snapshots.
- [x] Run full Vitest suite.
- [x] Run lint.
- [x] Run production build.
- [ ] Start local dev server if needed and visually inspect key screens.
- [ ] Capture after screenshots and compare against baseline for overlap, clipped text, unreadable contrast, broken dialogs, and broken terminal/database views.
- [x] Confirm no Tauri command names, invoke payloads, event listeners, or Rust files changed.

## Acceptance Criteria

- [x] Project remains Vite + React + Tauri.
- [x] `vite.config.ts`, `src-tauri/tauri.conf.json`, `src/main.tsx`, and Tauri API contracts are unchanged unless a strictly style-related import path adjustment is unavoidable.
- [x] No Next.js files are introduced: no `next.config.*`, no `app/` route migration, no Next scripts in `package.json`.
- [x] UI uses shadcn `b5xKlRYuO` visual language through tokens and shared controls.
- [x] Existing business behavior and API calls are unchanged.
- [x] `pnpm build` passes.
- [x] No unrelated existing git changes are reverted or overwritten.
