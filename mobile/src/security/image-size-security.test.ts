import { execFile } from "node:child_process";
import { basename, resolve } from "node:path";
import { promisify } from "node:util";
import { describe, expect, it } from "vitest";

const execFileAsync = promisify(execFile);
const mobileRoot = basename(process.cwd()) === "mobile" ? process.cwd() : resolve("mobile");

const malformedImages = {
  icns: Buffer.from([
    0x69,
    0x63,
    0x6e,
    0x73, // icns
    0x00,
    0x00,
    0x00,
    0x10, // file length
    0x69,
    0x63,
    0x30,
    0x37, // ic07 entry
    0x00,
    0x00,
    0x00,
    0x00, // invalid zero entry length
  ]),
  jxl: Buffer.from([
    0x00,
    0x00,
    0x00,
    0x0c,
    0x4a,
    0x58,
    0x4c,
    0x20,
    0x6a,
    0x78,
    0x6c,
    0x20, // ftyp
    0x00,
    0x00,
    0x00,
    0x00,
    0x6a,
    0x78,
    0x6c,
    0x70, // zero-sized jxlp box
  ]),
  heif: Buffer.from([
    0x00,
    0x00,
    0x00,
    0x0c,
    0x66,
    0x74,
    0x79,
    0x70,
    0x68,
    0x65,
    0x69,
    0x63, // ftyp
    0x00,
    0x00,
    0x00,
    0x00,
    0x66,
    0x72,
    0x65,
    0x65, // zero-sized box
  ]),
};

describe("patched image-size parsers", () => {
  for (const [format, input] of Object.entries(malformedImages)) {
    it(`rejects a non-advancing ${format} box without hanging`, async () => {
      const script = [
        'const imageSize = require("image-size");',
        `const input = Buffer.from("${input.toString("base64")}", "base64");`,
        "try { imageSize(input); process.exit(2); } catch { process.exit(0); }",
      ].join("\n");

      await expect(
        execFileAsync(process.execPath, ["-e", script], {
          cwd: mobileRoot,
          timeout: 1_000,
        }),
      ).resolves.toMatchObject({ stderr: "" });
    });
  }
});
