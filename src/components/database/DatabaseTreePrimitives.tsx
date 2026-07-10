import { ChevronDown, ChevronRight } from "lucide-react";
import type { AeroricDbConnectionConfig, DbConnectionConfig } from "../../types";
import s from "../../styles";
import { connectionBadgeColor, connectionBadgeText } from "./databaseSidebarTreeState";

export function ConnectionNameBadge({
  connection,
  size = 22,
}: {
  connection: DbConnectionConfig | AeroricDbConnectionConfig;
  size?: number;
}) {
  const text = connectionBadgeText(connection.name);
  const color = connectionBadgeColor(connection);
  return (
    <span
      aria-hidden="true"
      style={{
        ...s.databaseConnectionNameBadge,
        width: size,
        height: size,
        background: `${color}22`,
        border: `1px solid ${color}77`,
        color,
        fontSize: Math.max(9, size * (text.length > 2 ? 0.34 : 0.42)),
      }}
    >
      {text}
    </span>
  );
}

export function ExpansionGlyph({ expanded }: { expanded: boolean }) {
  const Icon = expanded ? ChevronDown : ChevronRight;
  return <Icon aria-hidden="true" size={11} style={s.databaseTreeChevron} />;
}
