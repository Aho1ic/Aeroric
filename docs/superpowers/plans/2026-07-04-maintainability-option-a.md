# Maintainability Option A Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve maintainability for recent agent settings UI code without changing user-facing behavior.

**Architecture:** Extract repeated UI and event wiring into focused helpers while keeping existing component boundaries and styles. The refactor stays in the frontend because the selected issues are frontend component duplication; backend Rust files are scanned but not modified in this pass.

**Tech Stack:** React, TypeScript, Vitest, Testing Library, Tauri event dispatch through browser `window`.

---

### Task 1: Centralize App Settings Open Event

**Files:**
- Modify: `src/components/app-settings/types.ts`
- Modify: `src/components/ProjectRail.tsx`
- Modify: `src/components/WelcomePage.tsx`
- Modify: `src/components/SidebarFooterActions.tsx`
- Test: `src/test/sidebar-footer-actions.test.tsx`
- Test: `src/test/project-rail-drag.test.tsx`

- [ ] **Step 1: Write or adjust tests**

Keep tests asserting that dispatching the settings event with `{ initialNav: "codex" }` opens the Codex settings tab, and that sidebar direct open resets to `general`.

- [ ] **Step 2: Run focused tests and verify current behavior**

Run:

```bash
pnpm vitest run src/test/sidebar-footer-actions.test.tsx src/test/project-rail-drag.test.tsx
```

Expected: existing tests pass before the refactor.

- [ ] **Step 3: Extract event helper**

Add this helper to `src/components/app-settings/types.ts`:

```ts
export function openAppSettings(initialNav?: NavKey) {
  window.dispatchEvent(
    new CustomEvent<OpenAppSettingsDetail>(OPEN_APP_SETTINGS_EVENT, {
      detail: initialNav ? { initialNav } : undefined,
    }),
  );
}
```

Replace local `new CustomEvent(OPEN_APP_SETTINGS_EVENT, ...)` call sites with `openAppSettings(...)`.

- [ ] **Step 4: Run focused tests**

Run:

```bash
pnpm vitest run src/test/sidebar-footer-actions.test.tsx src/test/project-rail-drag.test.tsx
```

Expected: all focused tests pass.

### Task 2: Extract Shared Confirm Dialog

**Files:**
- Create: `src/components/ui/ConfirmDialog.tsx`
- Modify: `src/components/app-settings/AgentConfigPanel.tsx`
- Test: `src/test/agent-config-debug-ui.test.tsx`

- [ ] **Step 1: Run existing delete-confirmation tests**

Run:

```bash
pnpm vitest run src/test/agent-config-debug-ui.test.tsx
```

Expected: existing delete confirmation tests pass before the refactor.

- [ ] **Step 2: Create focused dialog component**

Create a presentational `ConfirmDialog` with props:

```ts
type ConfirmDialogProps = {
  title: string;
  message: string;
  cancelLabel: string;
  confirmLabel: string;
  confirmingLabel?: string;
  confirming?: boolean;
  destructive?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
};
```

Use the same overlay, dialog sizing, colors, and `Button` variants currently used in `AgentConfigPanel`.

- [ ] **Step 3: Replace inline delete dialog**

Render `ConfirmDialog` from `AgentConfigPanel` when `deleteConfirmOpen` is true. Preserve the behavior that clicking the overlay cancels only when not deleting, and clicking confirm is the only path that calls `delete_custom_agent_profile`.

- [ ] **Step 4: Run focused tests**

Run:

```bash
pnpm vitest run src/test/agent-config-debug-ui.test.tsx
```

Expected: all focused tests pass.

### Task 3: Extract Project Rail Footer Actions

**Files:**
- Create: `src/components/project-rail/ProjectRailFooterActions.tsx`
- Modify: `src/components/ProjectRail.tsx`
- Test: `src/test/project-rail.test.ts`
- Test: `src/test/project-rail-drag.test.tsx`

- [ ] **Step 1: Run existing project rail tests**

Run:

```bash
pnpm vitest run src/test/project-rail.test.ts src/test/project-rail-drag.test.tsx
```

Expected: existing tests pass before the refactor.

- [ ] **Step 2: Move footer button rendering to a focused component**

Create `ProjectRailFooterActions` that receives translated labels, `themeVariant`, `singleProjectMode`, and handlers for back home, agent settings, open project, and theme toggle. It owns the hover state for footer buttons and renders the same button sequence in both collapsed and expanded layouts.

- [ ] **Step 3: Replace duplicated footer blocks**

In `ProjectRail.tsx`, remove the duplicated inline `footerIconButton` blocks and render `ProjectRailFooterActions` in both rail layouts. Keep `NotificationBell` styling unchanged.

- [ ] **Step 4: Run focused tests**

Run:

```bash
pnpm vitest run src/test/project-rail.test.ts src/test/project-rail-drag.test.tsx
```

Expected: all focused tests pass.

### Task 4: Final Verification and Review

**Files:**
- Review all changed files.

- [ ] **Step 1: Run full checks**

Run:

```bash
pnpm lint
pnpm test
pnpm build
git diff --check
```

Expected: lint, tests, build, and whitespace checks pass.

- [ ] **Step 2: Manual code review**

Review the final diff for behavior changes, unclear naming, excess abstraction, and missing test coverage. Fix any blocking issue before commit.

- [ ] **Step 3: Commit and push**

Run:

```bash
git add docs/superpowers/plans/2026-07-04-maintainability-option-a.md src
git commit -m "refactor: consolidate settings ui helpers"
git push origin main
```

Expected: commit succeeds on `main` and pushes to `origin/main`.
