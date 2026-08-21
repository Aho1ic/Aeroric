import golden from "@aeroric/remote-contracts/fixtures";
import { describe, expect, it } from "vitest";
import { extractPairingCode, parsePairingOffer } from "./pairing-offer";

function encodeOffer(offer: unknown): string {
  return Buffer.from(JSON.stringify(offer), "utf-8")
    .toString("base64")
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

const VALID_OFFER = {
  v: 2,
  endpoints: ["ws://192.168.1.10:6790"],
  invite: "invite-token-abc",
  hostName: "我的 Mac",
  hostId: "stable-host-id",
  publicKey: "static-public-key",
};

describe("extractPairingCode", () => {
  it("extracts code from deep link", () => {
    const code = encodeOffer(VALID_OFFER);
    expect(extractPairingCode(`aeroric://pair?code=${code}`)).toBe(code);
  });

  it("accepts bare base64url code", () => {
    const code = encodeOffer(VALID_OFFER);
    expect(extractPairingCode(`  ${code}  `)).toBe(code);
  });

  it("rejects other schemes and hosts", () => {
    expect(extractPairingCode("https://evil.example/pair?code=abc")).toBeNull();
    expect(extractPairingCode("aeroric://settings?code=abc")).toBeNull();
    expect(extractPairingCode("")).toBeNull();
  });
});

describe("parsePairingOffer", () => {
  it("round-trips a valid offer with unicode host name", () => {
    const offer = parsePairingOffer(`aeroric://pair?code=${encodeOffer(VALID_OFFER)}`);
    expect(offer.endpoints).toEqual(["ws://192.168.1.10:6790"]);
    expect(offer.invite).toBe("invite-token-abc");
    expect(offer.hostName).toBe("我的 Mac");
    expect(offer.hostId).toBe("stable-host-id");
    expect(offer.publicKey).toBe("static-public-key");
  });

  it("loads the optional confirmation capability from the shared fixture", () => {
    const offer = parsePairingOffer(encodeOffer(golden.pairingConfirmation.offer));

    expect(offer.pairingConfirmationVersion).toBe(1);
  });

  it("ignores an unknown future confirmation version and keeps legacy pairing", () => {
    const offer = parsePairingOffer(
      encodeOffer({ ...VALID_OFFER, pairingConfirmationVersion: 99 }),
    );

    expect(offer.pairingConfirmationVersion).toBeUndefined();
  });

  it("rejects unsupported version", () => {
    const code = encodeOffer({ ...VALID_OFFER, v: 99 });
    expect(() => parsePairingOffer(code)).toThrow(/版本不支持/);
  });

  it("tells the user to upgrade the desktop for legacy v1 offers", () => {
    const code = encodeOffer({ ...VALID_OFFER, v: 1 });
    expect(() => parsePairingOffer(code)).toThrow(/旧版桌面端/);
  });

  it("rejects offers without the host public key", () => {
    const code = encodeOffer({ ...VALID_OFFER, publicKey: "" });
    expect(() => parsePairingOffer(code)).toThrow(/主机公钥/);
  });

  it("rejects missing invite", () => {
    const code = encodeOffer({ ...VALID_OFFER, invite: "" });
    expect(() => parsePairingOffer(code)).toThrow(/邀请令牌/);
  });

  it("rejects non-ws endpoints", () => {
    const code = encodeOffer({ ...VALID_OFFER, endpoints: ["http://192.168.1.10:6790"] });
    expect(() => parsePairingOffer(code)).toThrow(/连接地址/);
  });

  it("rejects garbage input", () => {
    expect(() => parsePairingOffer("not-base64!!##")).toThrow();
    expect(() => parsePairingOffer(encodeOffer(null))).toThrow();
  });
});
