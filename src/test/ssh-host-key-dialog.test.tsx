import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { SshHostKeyDialog } from "../components/ssh/SshHostKeyDialog";
import { sshHostKeyGate } from "../components/ssh/session";
import type { SshConnection, SshHostKey } from "../types";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) => invokeMock(command, args),
  isTauri: () => false,
}));

const connection: SshConnection = {
  id: "conn-1",
  name: "oracle",
  host: "217.142.187.92",
  port: 22,
  username: "opc",
  createdAt: 1,
};

const keys: SshHostKey[] = [
  {
    keyType: "ssh-ed25519",
    fingerprint: "SHA256:QWYSzHIlnZ3CjHkjh54wW7fcxwtUu7cCq7u4kSrJang",
    knownHostsLine: "217.142.187.92 ssh-ed25519 AAAAC3Nza",
  },
  {
    keyType: "ecdsa-sha2-nistp256",
    fingerprint: "SHA256:3z4RpTmgBJK4SFIQ3F9eHHMEMN2G3ZgyfiQ8Ct80E64",
    knownHostsLine: "217.142.187.92 ecdsa-sha2-nistp256 AAAAE2Vj",
  },
];

describe("sshHostKeyGate", () => {
  it("only prompts for hosts that are absent from known_hosts", () => {
    expect(sshHostKeyGate({ state: "unknown", target: "host", keys })).toEqual({
      action: "prompt",
      target: "host",
      keys,
    });
    expect(sshHostKeyGate({ state: "trusted" })).toEqual({ action: "connect" });
  });

  /// 主机不通不是 host key 问题,必须放行让 ssh 报真实的连接错误。
  it("connects anyway when the host cannot be scanned", () => {
    expect(sshHostKeyGate({ state: "unreachable", target: "host" })).toEqual({
      action: "connect",
    });
  });
});

describe("SshHostKeyDialog", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("shows every offered fingerprint so the user can compare them", () => {
    render(
      <I18nProvider>
        <SshHostKeyDialog
          connection={connection}
          target="217.142.187.92"
          keys={keys}
          onTrusted={vi.fn()}
          onCancel={vi.fn()}
        />
      </I18nProvider>,
    );

    expect(
      screen.getByText("SHA256:QWYSzHIlnZ3CjHkjh54wW7fcxwtUu7cCq7u4kSrJang"),
    ).toBeInTheDocument();
    expect(screen.getByText("ssh-ed25519")).toBeInTheDocument();
    expect(
      screen.getByText("SHA256:3z4RpTmgBJK4SFIQ3F9eHHMEMN2G3ZgyfiQ8Ct80E64"),
    ).toBeInTheDocument();
    expect(screen.getByRole("dialog")).toHaveTextContent("217.142.187.92");
  });

  /// 后端要靠这份指纹清单做 TOCTOU 比对,漏传或传错就会写入用户没看过的 key。
  it("sends exactly the fingerprints it displayed when trusting", async () => {
    const user = userEvent.setup();
    invokeMock.mockResolvedValue(undefined);
    const onTrusted = vi.fn();

    render(
      <I18nProvider>
        <SshHostKeyDialog
          connection={connection}
          target="217.142.187.92"
          keys={keys}
          onTrusted={onTrusted}
          onCancel={vi.fn()}
        />
      </I18nProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Trust and connect" }));

    expect(invokeMock).toHaveBeenCalledWith("trust_ssh_host_key", {
      connection,
      approvedFingerprints: [
        "SHA256:QWYSzHIlnZ3CjHkjh54wW7fcxwtUu7cCq7u4kSrJang",
        "SHA256:3z4RpTmgBJK4SFIQ3F9eHHMEMN2G3ZgyfiQ8Ct80E64",
      ],
    });
    expect(onTrusted).toHaveBeenCalledOnce();
  });

  it("keeps the dialog open and surfaces the reason when trusting fails", async () => {
    const user = userEvent.setup();
    invokeMock.mockRejectedValue("217.142.187.92 now presents a different host key");
    const onTrusted = vi.fn();

    render(
      <I18nProvider>
        <SshHostKeyDialog
          connection={connection}
          target="217.142.187.92"
          keys={keys}
          onTrusted={onTrusted}
          onCancel={vi.fn()}
        />
      </I18nProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Trust and connect" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("now presents a different host key");
    expect(onTrusted).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("does not touch known_hosts when the user cancels", async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();

    render(
      <I18nProvider>
        <SshHostKeyDialog
          connection={connection}
          target="217.142.187.92"
          keys={keys}
          onTrusted={vi.fn()}
          onCancel={onCancel}
        />
      </I18nProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Cancel" }));

    expect(onCancel).toHaveBeenCalledOnce();
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
