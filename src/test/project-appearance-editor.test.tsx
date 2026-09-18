import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ProjectAppearanceEditor } from "../components/ProjectAppearanceEditor";
import { I18nProvider } from "../i18n";

function renderEditor(props: Partial<React.ComponentProps<typeof ProjectAppearanceEditor>> = {}) {
  const onSave = vi.fn();
  const onCancel = vi.fn();
  const { container } = render(
    <I18nProvider>
      <ProjectAppearanceEditor name="agent-config" onSave={onSave} onCancel={onCancel} {...props} />
    </I18nProvider>,
  );
  return { onSave, onCancel, container };
}

describe("ProjectAppearanceEditor", () => {
  it("saves the picked palette key", async () => {
    const user = userEvent.setup();
    const { onSave } = renderEditor();

    await user.click(screen.getByRole("button", { name: "Rose" }));
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(onSave).toHaveBeenCalledWith({ color: "rose" });
  });

  it("saves custom initials typed in the initials mode", async () => {
    const user = userEvent.setup();
    const { onSave } = renderEditor();

    await user.click(screen.getByRole("button", { name: "Initials" }));
    await user.type(screen.getByLabelText("Custom initials"), "ZZ");
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(onSave).toHaveBeenCalledWith({ label: "ZZ" });
  });

  it("saves an emoji and drops the initials the user had typed", async () => {
    const user = userEvent.setup();
    const { onSave } = renderEditor({ avatar: { label: "AB" } });

    await user.click(screen.getByRole("button", { name: "Emoji" }));
    await user.type(screen.getByLabelText("Emoji"), "🚀");
    await user.click(screen.getByRole("button", { name: "Save" }));

    // 内容是互斥三态:选了 emoji 就不该同时存 label。
    expect(onSave).toHaveBeenCalledWith({ emoji: "🚀" });
  });

  it("saves undefined when reset back to auto", async () => {
    const user = userEvent.setup();
    const { onSave } = renderEditor({ avatar: { color: "pink", emoji: "🚀" } });

    await user.click(screen.getByRole("button", { name: "Reset to auto" }));
    await user.click(screen.getByRole("button", { name: "Save" }));

    // undefined = 清除定制,与从未定制过同构。
    expect(onSave).toHaveBeenCalledWith(undefined);
  });

  it("opens in the mode implied by the stored override", () => {
    renderEditor({ avatar: { emoji: "🚀" } });

    expect(screen.getByRole("button", { name: "Emoji" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Auto" })).toHaveAttribute("aria-pressed", "false");
  });

  it("drives selection through AnimatedSelection, not hand-rolled highlight", () => {
    const { container } = renderEditor();

    // 互斥控件必须复用共享指示器:两组(颜色 + 内容)都在。
    expect(container.querySelectorAll(".animated-selection").length).toBe(2);
    expect(container.querySelectorAll("[data-animated-selection-item]").length).toBeGreaterThan(2);
  });
});
