import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const semverPattern = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;
const literalUserAgentPattern =
  /Aeroric\/\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?/g;

function readJson(relativePath) {
  return JSON.parse(readFileSync(join(repoRoot, relativePath), "utf8"));
}

function cargoPackageVersion() {
  const source = readFileSync(join(repoRoot, "src-tauri/Cargo.toml"), "utf8");
  const lines = source.split(/\r?\n/);
  const packageStart = lines.findIndex((line) => line.trim() === "[package]");
  const packageLines = [];
  for (let index = packageStart + 1; index >= 1 && index < lines.length; index += 1) {
    if (/^\s*\[/.test(lines[index])) break;
    packageLines.push(lines[index]);
  }
  const version = packageLines.join("\n").match(/^\s*version\s*=\s*"([^"]+)"/m)?.[1];
  if (!version) throw new Error("Could not read [package].version from src-tauri/Cargo.toml");
  return version;
}

function readMarkedVersion(relativePath, marker) {
  const source = readFileSync(join(repoRoot, relativePath), "utf8");
  const version = source.match(marker)?.[1];
  if (!version) throw new Error(`Could not find the current release marker in ${relativePath}`);
  return version;
}

function rustFilesUnder(relativePath) {
  const root = join(repoRoot, relativePath);
  const files = [];
  const visit = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) visit(path);
      else if (entry.isFile() && entry.name.endsWith(".rs")) files.push(path);
    }
  };
  visit(root);
  return files;
}

function parseTag(argv) {
  const index = argv.indexOf("--tag");
  if (index === -1) return undefined;
  const value = argv[index + 1];
  if (!value) throw new Error("--tag requires a value");
  return value.replace(/^v/, "");
}

const errors = [];
try {
  const packageVersion = readJson("package.json").version;
  const tauriVersion = readJson("src-tauri/tauri.conf.json").version;
  const cargoVersion = cargoPackageVersion();
  const readmeVersion = readMarkedVersion(
    "README.md",
    /<strong>Current release:<\/strong>\s*v([0-9A-Za-z.+-]+)/,
  );
  const readmeZhVersion = readMarkedVersion(
    "README_ZH.md",
    /<strong>当前版本：<\/strong>\s*v([0-9A-Za-z.+-]+)/,
  );
  const versions = {
    "package.json": packageVersion,
    "src-tauri/tauri.conf.json": tauriVersion,
    "src-tauri/Cargo.toml": cargoVersion,
    "README.md": readmeVersion,
    "README_ZH.md": readmeZhVersion,
  };

  for (const [source, version] of Object.entries(versions)) {
    if (typeof version !== "string" || !semverPattern.test(version)) {
      errors.push(`${source} has an invalid semver: ${String(version)}`);
    } else if (version !== packageVersion) {
      errors.push(`${source} is ${version}, expected ${packageVersion}`);
    }
  }

  const tag = parseTag(process.argv.slice(2));
  if (tag !== undefined && tag !== packageVersion) {
    errors.push(`release tag is ${tag}, expected ${packageVersion}`);
  }

  for (const file of rustFilesUnder("src-tauri/src")) {
    const source = readFileSync(file, "utf8");
    const literals = source.match(literalUserAgentPattern);
    if (literals) {
      errors.push(
        `${file.slice(repoRoot.length + 1)} contains hard-coded Aeroric User-Agent(s): ${[
          ...new Set(literals),
        ].join(", ")}`,
      );
    }
  }

  const skills = readFileSync(join(repoRoot, "src-tauri/src/skills.rs"), "utf8");
  if (!skills.includes('const AERORIC_USER_AGENT: &str = concat!("Aeroric/", env!("CARGO_PKG_VERSION"));')) {
    errors.push("src-tauri/src/skills.rs must derive AERORIC_USER_AGENT from CARGO_PKG_VERSION");
  }
} catch (error) {
  errors.push(error instanceof Error ? error.message : String(error));
}

if (errors.length > 0) {
  for (const error of errors) console.error(`::error::${error}`);
  process.exitCode = 1;
} else {
  console.log("Version consistency check passed.");
}
