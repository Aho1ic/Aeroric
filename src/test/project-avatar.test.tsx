import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ProjectAvatar } from "../components/ProjectAvatar";

describe("ProjectAvatar", () => {
  it("preserves project name casing in initials", () => {
    render(<ProjectAvatar name="aeroric" />);

    expect(screen.getByText("ae")).toBeInTheDocument();
    expect(screen.queryByText("AE")).not.toBeInTheDocument();
  });
});
