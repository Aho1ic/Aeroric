# Right Toolbar shadcn/ui Button Style Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans or inline execution for this scoped change. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将项目页面右侧垂直边栏中所有工具按钮调整为接近 shadcn/ui Button 的 `ghost` / `icon` 风格，同时保持现有交互逻辑不变。

**Architecture:** 右侧边栏按钮由 `src/components/RightToolbar.tsx` 统一渲染，并全部复用 `src/components/IconButton.tsx`。本次只修改 `IconButton` 的视觉状态与 `RightToolbar` 容器间距，不引入 Tailwind、CVA 或新的运行时依赖。

**Tech Stack:** React 19、TypeScript、Vite、Vitest、Testing Library、lucide-react、现有 CSS 变量主题系统。

---

## shadcn/ui 样式参考

- Button 组件定位：用于渲染原生按钮或看起来像按钮的组件。
- 右侧工具栏按钮映射：采用 shadcn/ui `ghost` + `icon` 的组合语义。
- 关键视觉特征：`inline-flex` 居中、`rounded-md`、`text-sm/font-medium` 的克制系统感、`transition-all`、禁用态 `opacity: 50%`、图标按钮固定尺寸、hover 使用弱背景、active 使用更明确的前景色和轻量背景。
- 本项目没有 Tailwind/CVA 基础设施，因此用当前 inline style 和设计 token 复刻，而不是引入完整 shadcn 组件体系。

## Scope

本次包含：

- 右侧垂直工具栏所有按钮：
  - 文件、Git 变更、Git 历史
  - IDE 工具注册表输出的 Git Advanced、Problems、Tests、Debug、Run、Preview、Search
  - SSH、SFTP、Database、Notes、Docker
  - Terminal
  - Settings
- `IconButton` 的默认、hover、active、disabled 四种视觉状态。
- 与按钮视觉一致性相关的右侧工具栏容器间距。
- 回归测试覆盖右侧工具栏按钮的样式契约。

本次不包含：

- 右侧面板内部业务按钮，例如文件浏览器排序按钮、Git 面板按钮、数据库面板按钮。
- 引入 Tailwind、`class-variance-authority`、`@radix-ui/react-slot` 或新 shadcn 目录。
- 修改按钮图标集合、面板切换逻辑、键盘快捷键或 i18n 文案。

## File Map

- Modify: `src/components/IconButton.tsx`
  - 统一右侧工具栏按钮的 shadcn-like icon button 样式。
  - 通过 `aria-pressed` 暴露 active 状态，提升可测性和可访问性。
- Modify: `src/components/RightToolbar.tsx`
  - 调整右侧工具栏宽度、padding、gap、分隔线，使 36px 图标按钮拥有稳定点击区域。
- Modify: `src/test/project-toolbar.test.tsx`
  - 更新现有右侧工具栏样式断言。
  - 增加 active / inactive / disabled 样式契约测试。
- Create: `doc/right-toolbar-shadcn-button-style-plan.md`
  - 保存本计划。

## Implementation Tasks

### Task 1: 写失败测试锁定新样式契约

- [ ] 修改 `src/test/project-toolbar.test.tsx` 中已有断言：
  - inactive 按钮背景应为 `transparent`。
  - active 按钮背景应为 `var(--accent-subtle)`。
  - active 按钮边框应为 `var(--accent-soft)`。
  - active 按钮颜色应为 `var(--accent-strong)`。
  - 所有右侧工具栏 icon button 应保持 `36px x 36px`。
- [ ] 增加 disabled 样式断言：
  - 传入 `dockerDisabled` 或 SSH 项目缺少远程连接时，按钮禁用态保留稳定尺寸。
  - 禁用态 `opacity` 为 `0.5`，cursor 为 `not-allowed`。
- [ ] 运行：
  - `pnpm vitest run src/test/project-toolbar.test.tsx`
  - 预期：新样式测试失败，因为当前 `IconButton` active 背景仍为 `none`，尺寸仍为默认 32px。

### Task 2: 修改 IconButton 为 shadcn-like ghost/icon 样式

- [ ] 在 `src/components/IconButton.tsx` 中保留现有 API，不改调用方。
- [ ] 将默认尺寸从 32 调整为 36，对应 shadcn `icon` 尺寸语义。
- [ ] 将按钮基础样式调整为：
  - `display: inline-flex`
  - `alignItems / justifyContent: center`
  - `borderRadius: "var(--radius-md)"`
  - `border: "1px solid transparent"`
  - `background: "transparent"`
  - `color: "var(--text-muted)"`
  - `transition: "background-color 0.15s ease, border-color 0.15s ease, color 0.15s ease, opacity 0.15s ease, box-shadow 0.15s ease"`
- [ ] hover 状态使用 shadcn `ghost` 风格：
  - background: `var(--bg-hover)`
  - color: `var(--text-primary)`
  - borderColor: `var(--border-dim)`
- [ ] active 状态使用本项目 accent token 表达 selected icon button：
  - background: `var(--accent-subtle)`
  - color: `var(--accent-strong)`
  - borderColor: `var(--accent-soft)`
  - boxShadow: `inset 0 0 0 1px color-mix(in srgb, var(--accent-soft) 55%, transparent)`
- [ ] disabled 状态：
  - `opacity: 0.5`
  - `cursor: "not-allowed"`
  - 不显示 hover。
- [ ] 添加 `aria-pressed={active || undefined}`。

### Task 3: 调整 RightToolbar 容器节奏

- [ ] 在 `src/components/RightToolbar.tsx` 中将容器宽度从 44 调整到 48。
- [ ] 将上下 padding 调整为 `8px 6px` 对应 36px 按钮和 4px gap。
- [ ] 将 `gap` 从 2 调整到 4，减少按钮之间的挤压感。
- [ ] 分隔线宽度保持 20 或调整到 22，高度 1，使用 `var(--border-dim)`，上下 margin 使用 `4px 0`。

### Task 4: 验证和回归

- [ ] 运行局部测试：
  - `pnpm vitest run src/test/project-toolbar.test.tsx`
  - 预期：全部通过。
- [ ] 运行全量类型和构建：
  - `pnpm build`
  - 预期：TypeScript 和 Vite build 通过。
- [ ] 如构建失败，优先修复由本次修改引入的问题；若失败来自既有环境或无关代码，记录具体错误。

## Design Notes

- 选择复用 `IconButton` 而不是在 `RightToolbar` 内为每个按钮单独写样式，因为当前右侧工具栏所有按钮已经汇聚到一个组件，修改集中且风险低。
- 选择 inline style 而不是引入 shadcn/ui 的 Tailwind 组件，因为仓库当前不是 Tailwind 架构，贸然引入会扩大改动面。
- active 状态不直接使用 shadcn 的 `default` 实心按钮，因为右侧工具栏是导航式 icon rail，`ghost + selected accent` 更接近 IDE 工具栏交互，也与现有主题 token 兼容。
- 保留 `title` tooltip 和所有点击处理，避免影响现有可用性。

## Self Review

- 没有 `TBD` / `TODO` 占位。
- 计划范围限定在右侧垂直工具栏，不包含右侧面板内部业务按钮。
- 测试先于实现，符合本次 TDD 流程。
- 不引入新依赖，避免破坏当前构建体系。
