import type { RedisKeyInfo } from "../types/database";

export interface RedisKeyTreeGroupNode {
  kind: "group";
  id: string;
  label: string;
  pathSegments: string[];
  children: RedisKeyTreeNode[];
}

export interface RedisKeyTreeLeafNode {
  kind: "leaf";
  id: string;
  label: string;
  fullKeyDisplay: string;
  keyRaw: string;
  keyType: string;
  ttl: number;
  size: number;
  valuePreview: string;
  pathSegments: string[];
}

export type RedisKeyTreeNode = RedisKeyTreeGroupNode | RedisKeyTreeLeafNode;

export interface RedisKeyTreeRow {
  node: RedisKeyTreeNode;
  depth: number;
}

function groupId(db: number, pathSegments: string[]) {
  return `redis-group:${db}:${pathSegments.join("\u0000")}`;
}

function leafId(db: number, keyRaw: string) {
  return `redis-leaf:${db}:${keyRaw}`;
}

function compareNodes(left: RedisKeyTreeNode, right: RedisKeyTreeNode) {
  if (left.kind !== right.kind) return left.kind === "group" ? -1 : 1;
  return left.label.localeCompare(right.label);
}

function sortNodes(nodes: RedisKeyTreeNode[]): RedisKeyTreeNode[] {
  return [...nodes].sort(compareNodes).map((node) =>
    node.kind === "group"
      ? {
          ...node,
          children: sortNodes(node.children),
        }
      : node,
  );
}

function insertKey(
  root: RedisKeyTreeNode[],
  groups: Map<string, RedisKeyTreeGroupNode>,
  key: RedisKeyInfo,
  db: number,
  separator: string,
) {
  const rawSegments = separator ? key.key_display.split(separator) : [key.key_display];
  const pathSegments = rawSegments.map((part) => part.trim()).filter(Boolean);
  const segments = pathSegments.length > 0 ? pathSegments : [key.key_display || key.key_raw];

  if (segments.length === 1) {
    root.push({
      kind: "leaf",
      id: leafId(db, key.key_raw),
      label: segments[0],
      fullKeyDisplay: key.key_display || key.key_raw,
      keyRaw: key.key_raw,
      keyType: key.key_type,
      ttl: key.ttl,
      size: key.size,
      valuePreview: key.value_preview,
      pathSegments: segments,
    });
    return;
  }

  let level = root;
  const groupSegments: string[] = [];
  for (const segment of segments.slice(0, -1)) {
    groupSegments.push(segment);
    const id = groupId(db, groupSegments);
    let group = groups.get(id);
    if (!group) {
      group = {
        kind: "group",
        id,
        label: segment,
        pathSegments: [...groupSegments],
        children: [],
      };
      groups.set(id, group);
      level.push(group);
    }
    level = group.children;
  }

  level.push({
    kind: "leaf",
    id: leafId(db, key.key_raw),
    label: segments[segments.length - 1],
    fullKeyDisplay: key.key_display || key.key_raw,
    keyRaw: key.key_raw,
    keyType: key.key_type,
    ttl: key.ttl,
    size: key.size,
    valuePreview: key.value_preview,
    pathSegments: segments,
  });
}

export function buildRedisKeyTree(
  keys: RedisKeyInfo[],
  db: number,
  separator = ":",
): RedisKeyTreeNode[] {
  const root: RedisKeyTreeNode[] = [];
  const groups = new Map<string, RedisKeyTreeGroupNode>();
  const seen = new Set<string>();

  for (const key of keys) {
    if (seen.has(key.key_raw)) continue;
    seen.add(key.key_raw);
    insertKey(root, groups, key, db, separator);
  }

  return sortNodes(root);
}

export function flattenVisibleRedisKeyTree(
  nodes: RedisKeyTreeNode[],
  expandedGroupIds: ReadonlySet<string>,
  depth = 0,
): RedisKeyTreeRow[] {
  const rows: RedisKeyTreeRow[] = [];
  for (const node of nodes) {
    rows.push({ node, depth });
    if (node.kind === "group" && expandedGroupIds.has(node.id)) {
      rows.push(...flattenVisibleRedisKeyTree(node.children, expandedGroupIds, depth + 1));
    }
  }
  return rows;
}

export function collectRedisGroupIds(nodes: RedisKeyTreeNode[]): Set<string> {
  const ids = new Set<string>();
  const visit = (items: RedisKeyTreeNode[]) => {
    for (const item of items) {
      if (item.kind !== "group") continue;
      ids.add(item.id);
      visit(item.children);
    }
  };
  visit(nodes);
  return ids;
}

export function countRedisTreeLeaves(node: RedisKeyTreeNode): number {
  if (node.kind === "leaf") return 1;
  return node.children.reduce((count, child) => count + countRedisTreeLeaves(child), 0);
}

export function collectRedisGroupKeyRaws(group: RedisKeyTreeGroupNode): string[] {
  const keys: string[] = [];
  const visit = (nodes: RedisKeyTreeNode[]) => {
    for (const node of nodes) {
      if (node.kind === "leaf") {
        keys.push(node.keyRaw);
      } else {
        visit(node.children);
      }
    }
  };
  visit(group.children);
  return keys;
}
