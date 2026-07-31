// Expo Go 扫码需要一个手机能直连的电脑 IPv4 地址。
// Clash/Surge 的 TUN 模式会创建 198.18.x / 198.19.x 的 fake-IP 接口，
// Expo 自动探测常常选中它，手机访问会超时。这里负责挑出真正的局域网地址。

const BENCHMARK_FIRST_OCTET = 198;
const BENCHMARK_SECOND_OCTETS = new Set([18, 19]);

/** 198.18.0.0/15 是基准测试保留段，Clash/Surge fake-IP 常用。 */
export function isBenchmarkAddress(address) {
  const [first, second] = address.split(".").map(Number);
  return first === BENCHMARK_FIRST_OCTET && BENCHMARK_SECOND_OCTETS.has(second);
}

/** RFC1918 私网 + CGNAT(100.64/10，Tailscale 常用)。 */
export function isPrivateAddress(address) {
  const [first, second] = address.split(".").map(Number);
  return (
    first === 10 ||
    (first === 172 && second >= 16 && second <= 31) ||
    (first === 192 && second === 168) ||
    (first === 100 && second >= 64 && second <= 127)
  );
}

/** 虚拟/隧道接口（VPN、容器、桥接等），手机通常无法直接访问。 */
export function isVirtualInterface(name) {
  return /^(anpi|ap|awdl|bridge|docker|gif|llw|lo|ppp|stf|tap|tun|utun|vEthernet|vmnet|wg)/i.test(
    name,
  );
}

/** 物理 Wi-Fi / 以太网接口（macOS en0…、Linux eth0/wlan0）。 */
export function isPhysicalInterface(name) {
  return /^(en\d|eth\d|wl)/i.test(name);
}

/** 分数越低越优先。 */
export function candidateScore(candidate) {
  let score = 0;
  if (isBenchmarkAddress(candidate.address)) score += 1000;
  if (isVirtualInterface(candidate.name)) score += 100;
  if (!isPrivateAddress(candidate.address)) score += 50;
  if (candidate.address.startsWith("192.168.")) score -= 20;
  if (isPhysicalInterface(candidate.name)) score -= 10;
  return score;
}

/**
 * 把 os.networkInterfaces() 的结果整理成按可用性排序的候选列表。
 * @param {Record<string, Array<{ address: string; family: string | number; internal: boolean }>>} interfaces
 */
export function rankCandidates(interfaces) {
  const candidates = [];
  const seen = new Set();

  for (const [name, entries] of Object.entries(interfaces ?? {})) {
    for (const entry of entries ?? []) {
      const family = typeof entry.family === "string" ? entry.family : `IPv${entry.family}`;
      if (family !== "IPv4" || entry.internal || seen.has(entry.address)) continue;
      seen.add(entry.address);
      candidates.push({ name, address: entry.address });
    }
  }

  return candidates.sort(
    (left, right) =>
      candidateScore(left) - candidateScore(right) ||
      left.name.localeCompare(right.name) ||
      left.address.localeCompare(right.address),
  );
}

/** 候选项后缀说明，例如 " [推荐，局域网]"。 */
export function candidateLabel(candidate, recommended) {
  const labels = [];
  if (recommended) labels.push("推荐");
  if (isBenchmarkAddress(candidate.address)) labels.push("代理 fake-IP，手机无法访问");
  else if (isVirtualInterface(candidate.name)) labels.push("VPN/虚拟接口");
  else if (isPrivateAddress(candidate.address)) labels.push("局域网");
  return labels.length > 0 ? ` [${labels.join("，")}]` : "";
}

/**
 * 判断本次 expo 调用是否需要让用户选择 LAN 地址。
 * 只有 `start` 且最终走 LAN 模式（默认、--lan、--offline、--host lan）才需要；
 * --tunnel / --localhost / 纯 web 不依赖局域网地址。
 * @param {string[]} cliArgs
 */
export function shouldPromptForLanAddress(cliArgs) {
  if (cliArgs[0] !== "start") return false;

  // expo start 未指定 host 时默认就是 lan。
  let hostMode = "lan";
  let webOnly = false;

  for (let index = 0; index < cliArgs.length; index += 1) {
    const arg = cliArgs[index];
    if (arg === "--tunnel") hostMode = "tunnel";
    else if (arg === "--localhost") hostMode = "localhost";
    else if (arg === "--lan" || arg === "--offline") hostMode = "lan";
    else if (arg === "--web" || arg === "-w") webOnly = true;
    else if (arg === "--host" || arg === "-m") hostMode = cliArgs[index + 1] ?? hostMode;
    else if (arg.startsWith("--host=")) hostMode = arg.slice("--host=".length);
    else if (arg === "--help" || arg === "-h") return false;
  }

  return hostMode === "lan" && !webOnly;
}
