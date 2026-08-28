/* 扫描依赖包里会触发 CSP 违规的构造。
 *
 * 背景:Aeroric 的 CSP 是 `script-src 'self'`,既没有 `unsafe-eval` 也没有
 * `wasm-unsafe-eval`。P1 要引入 mermaid / katex 做渲染,得先确认它们不需要
 * 这两条 —— 否则「前端渲染管线」这个决策不成立,Mermaid 要降级或改走 Rust。
 *
 * 用 Node 扫而不是 shell:`new Function` 和 `eval(` 里的括号和引号在 shell
 * 里要转义好几层,上一轮就是在这上面翻的车,拿到的是空结果而不是可信结论。
 *
 * 用法:node scripts/scan-csp-hazards.mjs <包名> [包名...]
 */

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

/** CSP 相关的危险构造。命中不等于一定违规(可能在死代码里),但需要人看一眼。 */
const HAZARDS = [
  // `new Function(...)` —— 需要 unsafe-eval
  { name: "new Function", re: /\bnew\s+Function\s*\(/g, needs: "unsafe-eval" },
  // 直接 eval 调用。排除 `.eval(`(可能是别的对象的方法)和 `//` 注释里的。
  { name: "eval()", re: /(?<![.\w$])eval\s*\(/g, needs: "unsafe-eval" },
  // WebAssembly 实例化 —— 需要 wasm-unsafe-eval
  {
    name: "WebAssembly",
    re: /\bWebAssembly\s*\.\s*(?:instantiate|compile|Module|Instance)\b/g,
    needs: "wasm-unsafe-eval",
  },
  // setTimeout/setInterval 传字符串等价于 eval。只标形如 setTimeout("...") 的。
  {
    name: "setTimeout(string)",
    re: /\bset(?:Timeout|Interval)\s*\(\s*["'`]/g,
    needs: "unsafe-eval",
  },
];

function walkJs(dir, out = []) {
  let entries;
  try {
    entries = readdirSync(dir);
  } catch {
    return out;
  }
  for (const entry of entries) {
    const full = join(dir, entry);
    let st;
    try {
      st = statSync(full);
    } catch {
      continue;
    }
    if (st.isDirectory()) {
      // 不进 node_modules 的嵌套依赖:那些包各自单独扫。
      if (entry === "node_modules") continue;
      walkJs(full, out);
    } else if (/\.(?:js|mjs|cjs)$/.test(entry)) {
      out.push({ path: full, size: st.size });
    }
  }
  return out;
}

/** 取命中处的上下文,便于判断是真调用还是字符串/注释。 */
function contextAt(text, index, span = 90) {
  const start = Math.max(0, index - span);
  const end = Math.min(text.length, index + span);
  return text.slice(start, end).replace(/\s+/g, " ").trim();
}

/**
 * 解析包的真实目录。
 *
 * pnpm 只把直接依赖软链到 `node_modules/<name>`;传递依赖只存在于
 * `node_modules/.pnpm/<name>@<version>/node_modules/<name>`。直接拼
 * `node_modules/<name>` 会对传递依赖得到「0 个文件」这种**假阴性** ——
 * 比「扫不到」更糟的是它长得像「扫过了,没问题」。
 */
function resolvePackageDirs(pkg) {
  const direct = join("node_modules", pkg);
  try {
    if (statSync(direct).isDirectory()) return [direct];
  } catch {
    // 落到 .pnpm 里找
  }

  const store = join("node_modules", ".pnpm");
  let entries;
  try {
    entries = readdirSync(store);
  } catch {
    return [];
  }
  // 目录名把 scope 的 `/` 编码成 `+`:`@scope/name@1.0.0` → `@scope+name@1.0.0`
  const encoded = pkg.replace("/", "+");
  const prefix = `${encoded}@`;
  const found = [];
  for (const entry of entries) {
    if (!entry.startsWith(prefix)) continue;
    // 排除 `foo@1.0.0_peer@2.0.0` 之外的同前缀不同包(如 `d3-array@` 撞 `d3@`)——
    // 版本号必须紧跟 `@`,所以下一个字符应是数字。
    const rest = entry.slice(prefix.length);
    if (!/^\d/.test(rest)) continue;
    const dir = join(store, entry, "node_modules", pkg);
    try {
      if (statSync(dir).isDirectory()) found.push(dir);
    } catch {
      // 该 store 条目没有这个包,跳过
    }
  }
  return found;
}

const packages = process.argv.slice(2);
if (packages.length === 0) {
  console.error("usage: node scripts/scan-csp-hazards.mjs <package> [package...]");
  process.exit(2);
}

let grandTotal = 0;
const unresolved = [];

for (const pkg of packages) {
  const dirs = resolvePackageDirs(pkg);
  if (dirs.length === 0) {
    // 解析不到必须显式报错,不能静静输出「0 个文件 ✅」。
    unresolved.push(pkg);
    console.log(`\n=== ${pkg} ===`);
    console.log("  ❌ 找不到该包 —— 未扫描(不是「没问题」)");
    continue;
  }
  const files = dirs.flatMap((dir) => walkJs(dir));
  console.log(`\n=== ${pkg} — ${files.length} 个 JS 文件(${dirs.length} 个位置)===`);

  const findings = new Map();
  for (const file of files) {
    let text;
    try {
      text = readFileSync(file.path, "utf8");
    } catch {
      continue;
    }
    for (const hazard of HAZARDS) {
      hazard.re.lastIndex = 0;
      let match;
      while ((match = hazard.re.exec(text)) !== null) {
        const list = findings.get(hazard.name) ?? [];
        list.push({
          // 把冗长的 .pnpm 前缀压掉,只留包内相对路径
          file: file.path.replace(/^.*?node_modules\//, "").replace(`${pkg}/`, ""),
          needs: hazard.needs,
          context: contextAt(text, match.index),
        });
        findings.set(hazard.name, list);
      }
    }
  }

  if (findings.size === 0) {
    console.log("  未发现 CSP 危险构造 ✅");
    continue;
  }

  for (const [name, hits] of findings) {
    grandTotal += hits.length;
    console.log(`\n  ⚠️  ${name} — ${hits.length} 处(需要 ${hits[0].needs})`);
    // 每类最多列 5 处,够判断性质了。
    for (const hit of hits.slice(0, 5)) {
      console.log(`     ${hit.file}`);
      console.log(`       …${hit.context}…`);
    }
    if (hits.length > 5) console.log(`     (另有 ${hits.length - 5} 处)`);
  }
}

console.log(`\n总计 ${grandTotal} 处命中。`);
if (unresolved.length > 0) {
  console.log(`⚠️  ${unresolved.length} 个包没扫到:${unresolved.join(", ")}`);
  // 非零退出:「没扫到」不能被当成通过。
  process.exit(1);
}
