import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { SftpPanel } from "../components/sftp/SftpPanel";
import type { SshConnection } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue([]),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  confirm: vi.fn().mockResolvedValue(true),
}));

const connections: SshConnection[] = [
  {
    id: "conn-1",
    name: "Staging",
    host: "staging.example.com",
    port: 22,
    username: "deploy",
    remotePath: "/srv/staging",
    createdAt: 1,
  },
  {
    id: "conn-2",
    name: "Production",
    host: "prod.example.com",
    port: 22,
    username: "deploy",
    remotePath: "/srv/app",
    createdAt: 2,
  },
];

describe("SftpPanel", () => {
  it("defaults to Local on the left and the current SSH project connection on the right", () => {
    render(
      <I18nProvider>
        <SftpPanel
          sshConnections={connections}
          localDefaultPath="/Users/me"
          active
          themeVariant="light"
          currentSshConnectionId="conn-2"
        />
      </I18nProvider>,
    );

    const triggers = screen.getAllByLabelText("Location");
    expect(triggers[0]).toHaveTextContent("Local");
    expect(triggers[1]).toHaveTextContent("Production");
    expect(screen.getByDisplayValue("/srv/app")).toBeInTheDocument();
  });
});
