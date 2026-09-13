import { describe, expect, it } from "vitest";
import type { SshConnection } from "../types";
import { sshConnectionCommand, sshConnectionUrl } from "../components/ssh/sshConnectionActions";

const connection: SshConnection = {
  id: "conn-1",
  name: "Production",
  host: "example.com",
  port: 2222,
  username: "deploy",
  identityFile: "/Users/me/key file",
  remotePath: "/srv/aeroric",
  createdAt: 1,
};

describe("SSH connection actions", () => {
  it("builds a shareable SSH URL with the configured remote path", () => {
    expect(sshConnectionUrl(connection)).toBe("ssh://deploy@example.com:2222/srv/aeroric");
  });

  it("builds a shell-safe SSH command with port and identity options", () => {
    expect(sshConnectionCommand(connection)).toBe(
      "ssh -i '/Users/me/key file' -p 2222 deploy@example.com",
    );
  });

  it("builds a paste-ready SSH command that never carries the password", () => {
    // 复制命令不夹带 SSHPASS:整串命令进剪贴板等于把明文交给任意能读
    // 剪贴板的进程。即使连接存了密码,命令也必须是纯 ssh 调用。
    expect(
      sshConnectionCommand({
        ...connection,
        host: "10.0.0.8",
        username: "root",
        hasPassword: true,
      }),
    ).toBe("ssh -i '/Users/me/key file' -p 2222 root@10.0.0.8");
  });
});
