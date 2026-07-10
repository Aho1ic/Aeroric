import { useState, type ReactNode } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { DbxButton } from "./DbxButton";
import {
  redisJsonChildNodes,
  redisJsonIsContainer,
  redisJsonNodeSummary,
  redisJsonScalarColor,
  redisJsonScalarText,
  type RedisJsonNode,
} from "./redisBrowserState";

export function RedisJsonTree({ value, wordWrap }: { value: unknown; wordWrap: boolean }) {
  const [collapsedPaths, setCollapsedPaths] = useState<Set<string>>(new Set());
  const togglePath = (path: string) => {
    setCollapsedPaths((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };
  const renderNode = (node: RedisJsonNode): ReactNode => {
    const children = redisJsonChildNodes(node);
    const container = redisJsonIsContainer(node.value);
    const collapsed = collapsedPaths.has(node.path);
    const bracketOpen = Array.isArray(node.value) ? "[" : "{";
    const bracketClose = Array.isArray(node.value) ? "]" : "}";
    return (
      <div key={node.path}>
        <div
          style={{
            minHeight: 24,
            display: "flex",
            alignItems: "flex-start",
            gap: 4,
            paddingLeft: node.depth * 16,
          }}
        >
          {container ? (
            <DbxButton
              variant="ghost"
              size="icon-xs"
              icon={collapsed ? ChevronRight : ChevronDown}
              onClick={() => togglePath(node.path)}
              aria-label={collapsed ? `Expand JSON ${node.path}` : `Collapse JSON ${node.path}`}
              style={{
                width: 18,
                height: 18,
                marginTop: 1,
                flex: "0 0 auto",
                color: "var(--text-muted)",
              }}
            />
          ) : (
            <span style={{ width: 18, flex: "0 0 auto" }} />
          )}
          {node.parentKind !== "root" && (
            <>
              <span
                style={{ color: node.parentKind === "array" ? "var(--text-muted)" : "#1d4ed8" }}
              >
                {node.parentKind === "array" ? `[${node.label}]` : JSON.stringify(node.label)}
              </span>
              <span style={{ color: "var(--text-muted)" }}>:</span>
            </>
          )}
          {container ? (
            <>
              <span style={{ fontWeight: 650, color: "var(--text-primary)" }}>{bracketOpen}</span>
              <span style={{ color: "var(--text-muted)" }}>{redisJsonNodeSummary(node.value)}</span>
              <span style={{ fontWeight: 650, color: "var(--text-primary)" }}>{bracketClose}</span>
            </>
          ) : (
            <span
              style={{
                color: redisJsonScalarColor(node.value),
                fontStyle: node.value === null ? "italic" : undefined,
              }}
            >
              {redisJsonScalarText(node.value)}
            </span>
          )}
        </div>
        {container && !collapsed && children.length > 0 && <div>{children.map(renderNode)}</div>}
      </div>
    );
  };
  return (
    <div
      role="tree"
      aria-label="Redis JSON tree"
      style={{
        whiteSpace: wordWrap ? "pre-wrap" : "pre",
        overflowWrap: wordWrap ? "anywhere" : "normal",
        color: "var(--text-primary)",
      }}
    >
      {renderNode({ key: "$", label: "$", value, path: "$", depth: 0, parentKind: "root" })}
    </div>
  );
}
