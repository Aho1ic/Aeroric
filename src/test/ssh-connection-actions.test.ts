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

  it("builds a paste-ready password SSH command with the complete connection target", () => {
    expect(
      sshConnectionCommand({
        ...connection,
        host: "10.0.0.8",
        username: "root",
        password: " s3c'ret value ",
      }),
    ).toBe(
      "env SSHPASS=' s3c'\\''ret value ' sshpass -e ssh -o PreferredAuthentications=password,keyboard-interactive -o PubkeyAuthentication=no -i '/Users/me/key file' -p 2222 root@10.0.0.8",
    );
  });
});
