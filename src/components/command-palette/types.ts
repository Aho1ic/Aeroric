export type CommandPaletteMode = "file" | "command";

export type CommandPaletteItemKind = "file" | "command";

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
