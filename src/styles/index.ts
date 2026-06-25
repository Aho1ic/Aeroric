import type React from "react";

import { common } from "./common";
import { database } from "./database";
import { dialogs } from "./dialogs";
import { font } from "./font";
import { gitDiff } from "./git-diff";
import { layout } from "./layout";
import { panels } from "./panels";
import { skillHub } from "./skill-hub";
import { task } from "./task";
import { terminal } from "./terminal";
import { timeline } from "./timeline";

const s = {
  ...layout,
  ...panels,
  ...terminal,
  ...dialogs,
  ...task,
  ...gitDiff,
  ...common,
  ...database,
  ...font,
  ...timeline,
  ...skillHub,
} satisfies Record<string, React.CSSProperties>;

export default s;

export {
  common,
  database,
  dialogs,
  font,
  gitDiff,
  layout,
  panels,
  skillHub,
  task,
  terminal,
  timeline,
};
