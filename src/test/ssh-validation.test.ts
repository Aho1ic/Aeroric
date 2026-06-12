import { describe, expect, it } from "vitest";
import {
  normalizeSshConnectionDraft,
  normalizeSshPort,
  validateSshConnectionDraft,
} from "../components/ssh/validation";

describe("normalizeSshPort", () => {
  it("defaults empty input to 22", () => {
    expect(normalizeSshPort("")).toBe(22);
    expect(normalizeSshPort("   ")).toBe(22);
  });

  it("rejects ports outside the valid TCP range", () => {
    expect(normalizeSshPort("0")).toBeNull();
    expect(normalizeSshPort("65536")).toBeNull();
  });

  it("accepts valid custom ports", () => {
    expect(normalizeSshPort("2200")).toBe(2200);
    expect(normalizeSshPort("65535")).toBe(65535);
  });
});

describe("validateSshConnectionDraft", () => {
  it("requires name, host, username, and a valid port", () => {
    expect(
      validateSshConnectionDraft({
        name: "",
        host: "",
          port: "70000",
          username: "",
          identityFile: "",
          password: "",
          remotePath: "",
        }),
    ).toEqual({
      name: "Name is required.",
      host: "Host is required.",
      port: "Port must be between 1 and 65535.",
      username: "Username is required.",
    });
  });
});

describe("normalizeSshConnectionDraft", () => {
  it("trims text fields and omits empty optional fields", () => {
    expect(
      normalizeSshConnectionDraft(
        {
          name: " prod ",
          host: " server.example.com ",
          port: " 2200 ",
          username: " deploy ",
          identityFile: " ",
          password: " ",
          remotePath: " /srv/app ",
        },
        123,
        456,
      ),
    ).toEqual({
      id: "123",
      name: "prod",
      host: "server.example.com",
      port: 2200,
      username: "deploy",
      remotePath: "/srv/app",
      createdAt: 456,
    });
  });

  it("preserves an explicit password while omitting blank passwords", () => {
    expect(
      normalizeSshConnectionDraft(
        {
          name: "prod",
          host: "server.example.com",
          port: "22",
          username: "deploy",
          identityFile: "",
          password: " secret ",
          remotePath: "",
        },
        123,
        456,
      ),
    ).toMatchObject({ password: "secret" });

    expect(
      normalizeSshConnectionDraft(
        {
          name: "prod",
          host: "server.example.com",
          port: "22",
          username: "deploy",
          identityFile: "",
          password: " ",
          remotePath: "",
        },
        123,
        456,
      ),
    ).not.toHaveProperty("password");
  });
});
