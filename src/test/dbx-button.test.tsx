import { render, screen } from "@testing-library/react";
import { Copy, Plus } from "lucide-react";
import { describe, expect, it } from "vitest";
import {
  DbxButton,
  DbxButtonGroup,
  DbxDialogFooterButton,
  DbxIconButton,
  DbxMenuItem,
  DbxSegmentedButton,
} from "../components/database/DbxButton";

describe("DbxButton", () => {
  it("renders dbx-sized icon-only buttons", () => {
    render(<DbxIconButton icon={Plus} aria-label="Add layer" size="icon-xs" />);

    expect(screen.getByRole("button", { name: "Add layer" })).toHaveStyle({
      width: "24px",
      height: "24px",
    });
  });

  it("exposes grouped and segmented button primitives", () => {
    render(
      <DbxButtonGroup aria-label="View mode">
        <DbxSegmentedButton active icon={Copy}>
          Icon
        </DbxSegmentedButton>
        <DbxSegmentedButton>List</DbxSegmentedButton>
      </DbxButtonGroup>,
    );

    expect(screen.getByRole("group", { name: "View mode" })).toHaveAttribute(
      "data-slot",
      "button-group",
    );
    expect(screen.getByRole("button", { name: /icon/i })).toHaveAttribute("aria-pressed", "true");
  });

  it("renders menu items and footer buttons with dbx variants", () => {
    render(
      <>
        <DbxMenuItem icon={Copy}>Copy name</DbxMenuItem>
        <DbxDialogFooterButton variant="destructive">Delete</DbxDialogFooterButton>
        <DbxButton disabled>Disabled</DbxButton>
      </>,
    );

    expect(screen.getByRole("menuitem", { name: /copy name/i })).toHaveStyle({ height: "28px" });
    expect(screen.getByRole("button", { name: "Delete" })).toHaveStyle({ height: "28px" });
    expect(screen.getByRole("button", { name: "Disabled" })).toBeDisabled();
  });
});
