export type CommandPaletteMode = "file" | "command" | "documentSymbol" | "workspaceSymbol";

export type CommandPaletteItemKind = "file" | "command" | "documentSymbol" | "workspaceSymbol";

export type CommandPaletteItem = {
  id: string;
  title: string;
  subtitle?: string;
  kind: CommandPaletteItemKind;
  keywords?: string[];
};

export type CommandPaletteParsedInput = {
  mode: CommandPaletteMode;
  query: string;
};
