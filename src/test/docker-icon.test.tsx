import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { DockerIcon } from "../components/DockerIcon";

describe("DockerIcon", () => {
  it("inherits toolbar button color instead of hardcoding Docker blue", () => {
    render(
      <div style={{ color: "rgb(65, 72, 87)" }}>
        <DockerIcon />
      </div>,
    );

    const icon = screen.getByTestId("docker-logo-icon");

    expect(icon).not.toHaveStyle({ color: "#2496ED" });
    expect(icon.querySelectorAll('[fill="currentColor"]').length).toBeGreaterThan(0);
  });

  it("uses the reference Docker mark shape with thirteen container blocks", () => {
    render(<DockerIcon />);

    const icon = screen.getByTestId("docker-logo-icon");

    expect(icon).toHaveAttribute("viewBox", "0 0 32 24");
    expect(icon.querySelectorAll('[data-docker-block="true"]')).toHaveLength(13);
    expect(icon.querySelector('[data-docker-hull="true"]')).toBeInTheDocument();
    expect(icon.querySelector('[data-docker-whale="true"]')).toBeInTheDocument();
  });
});
