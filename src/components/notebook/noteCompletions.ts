/* 触发式菜单的候选表:`[[` 笔记、`#` 标签、`@` 提及、`:` emoji。
 *
 * 纯函数,不碰 IPC 也不碰 DOM。检测在 `noteTriggers.ts`,菜单在 `NoteTriggerMenu.tsx`。
 * 打分复用命令面板那份(`noteCommands.ts` 的 `scoreFuzzyMatch`)—— 两个菜单用两套
 * 匹配规则的话,同一个查询在 ⌘K 里排第一、在 `[[` 里排第五,而这种不一致没人会
 * 往"有两份打分"上想。
 *
 * ## 标签和提及的候选从哪来
 *
 * 标签有后端全库扫描(`vaultTags`),但那个扫描很贵,面板只在侧栏对应档打开时才跑。
 * 补全不能要求"侧栏得先开着",所以这里接受**两个来源**并合并:已有的全库扫描结果
 * (有就用)+ 当前笔记正文里现写的那些。后者保证"我上一行刚写的 `#project` 这一行
 * 能补出来" —— 那是补全最常用的场景,而它恰好不依赖任何扫描。
 *
 * 提及(`@`)没有对应的后端扫描:Aeroric 没有人员 / 仓库索引,编造一个假的来源不如
 * 如实只给"这篇笔记里已经用过的 `@xxx`"。用户第一次写某个名字时没有候选,这是对的
 * —— 那个名字此刻确实还不存在于任何地方。
 */

import { scoreFuzzyMatch } from "./noteCommands";
import { normalizeTag } from "./noteTags";
import type { TriggerKind } from "./noteTriggers";

/** 菜单里的一条候选。 */
export type CompletionItem = {
  /** 去重与 React key 用。同一个菜单内唯一。 */
  id: string;
  /** 主行文字。 */
  label: string;
  /** 副行文字(路径 / 释义)。没有就不显示第二行。 */
  detail?: string;
  /** 左侧的字符图标(emoji 或 `#` `@` 这种符号)。 */
  glyph?: string;
  /** 提交时**替换掉触发序列**的文本。含末尾空格的由这里自己带。 */
  insert: string;
  /** 命中区间(码点下标,左闭右开),画高亮用。 */
  spans: { from: number; to: number }[];
};

/** 一次候选计算的输入。 */
export type CompletionSource = {
  kind: Exclude<TriggerKind, "slash">;
  query: string;
  /** 全部笔记(`[[` 用)。 */
  notes: { id: string; title: string }[];
  /** 已知的全库标签(原始大小写)。没有扫描结果就传空数组。 */
  vaultTags: string[];
  /** 当前笔记正文。用来补出"这篇里已经写过的"标签与提及。 */
  body: string;
};

/** 一次最多给多少条。菜单是浮层,再多也看不见,而打分要跑满全表。 */
export const COMPLETION_LIMIT = 20;

/** 扫正文找 token 的长度上限。超过就不扫 —— 每次按键都要重算,长文会卡。 */
const BODY_SCAN_LIMIT = 200_000;

/**
 * emoji 候选表。
 *
 * 关键词带中英两套:`:笑` 和 `:smile` 都该找到 😀。这里不走 i18n —— 一份表里同时
 * 认两种语言,比"跟着界面语言切换"更符合实际输入习惯(中文界面里的人照样会打
 * `:fire`,那是从 GitHub 带来的肌肉记忆)。
 */
const EMOJIS: { emoji: string; code: string; words: string[] }[] = [
  { emoji: "😀", code: "smile", words: ["笑", "开心", "grin"] },
  { emoji: "😂", code: "joy", words: ["笑哭", "大笑"] },
  { emoji: "🙂", code: "slight_smile", words: ["微笑"] },
  { emoji: "😅", code: "sweat_smile", words: ["汗", "尴尬"] },
  { emoji: "🤔", code: "thinking", words: ["思考", "想"] },
  { emoji: "😭", code: "sob", words: ["哭", "难过"] },
  { emoji: "😡", code: "rage", words: ["生气", "愤怒"] },
  { emoji: "👍", code: "thumbsup", words: ["赞", "好", "+1"] },
  { emoji: "👎", code: "thumbsdown", words: ["差", "-1"] },
  { emoji: "🙏", code: "pray", words: ["拜托", "感谢"] },
  { emoji: "👀", code: "eyes", words: ["看", "关注"] },
  { emoji: "🎉", code: "tada", words: ["庆祝", "发布"] },
  { emoji: "🚀", code: "rocket", words: ["发布", "上线", "火箭"] },
  { emoji: "🔥", code: "fire", words: ["热", "火"] },
  { emoji: "✨", code: "sparkles", words: ["新功能", "闪"] },
  { emoji: "💡", code: "bulb", words: ["灵感", "想法", "点子"] },
  { emoji: "⭐", code: "star", words: ["星", "收藏"] },
  { emoji: "✅", code: "white_check_mark", words: ["完成", "对", "done"] },
  { emoji: "❌", code: "x", words: ["错", "失败", "取消"] },
  { emoji: "⚠️", code: "warning", words: ["警告", "注意"] },
  { emoji: "❓", code: "question", words: ["疑问", "问题"] },
  { emoji: "🐛", code: "bug", words: ["bug", "缺陷", "虫"] },
  { emoji: "🔧", code: "wrench", words: ["修", "工具", "扳手"] },
  { emoji: "🔨", code: "hammer", words: ["构建", "锤"] },
  { emoji: "📝", code: "memo", words: ["笔记", "记录", "备忘"] },
  { emoji: "📌", code: "pushpin", words: ["置顶", "钉"] },
  { emoji: "📅", code: "date", words: ["日期", "日历"] },
  { emoji: "⏰", code: "alarm_clock", words: ["提醒", "闹钟", "时间"] },
  { emoji: "🔗", code: "link", words: ["链接", "引用"] },
  { emoji: "📎", code: "paperclip", words: ["附件", "夹"] },
  { emoji: "🔍", code: "mag", words: ["搜索", "查找"] },
  { emoji: "🏷️", code: "label", words: ["标签", "分类"] },
  { emoji: "💬", code: "speech_balloon", words: ["评论", "讨论"] },
  { emoji: "📦", code: "package", words: ["包", "依赖", "发布"] },
  { emoji: "🌱", code: "seedling", words: ["发芽", "新建", "种子"] },
  { emoji: "🧠", code: "brain", words: ["脑", "思路"] },
  { emoji: "⚡", code: "zap", words: ["性能", "快", "闪电"] },
  { emoji: "🔒", code: "lock", words: ["锁", "安全", "私密"] },
  { emoji: "🗑️", code: "wastebasket", words: ["删除", "垃圾桶"] },
  { emoji: "♻️", code: "recycle", words: ["重构", "回收"] },
];

/** 从正文里扫出 `#标签` / `@提及` 的 token。口径同 `noteTriggers.ts` 的触发规则。 */
function tokensInBody(body: string, prefix: "#" | "@"): string[] {
  if (body.length > BODY_SCAN_LIMIT) return [];
  const chars = prefix === "#" ? "\\p{L}\\p{N}_/-" : "\\p{L}\\p{N}_-";
  // 前面只允许行首或空白 —— 和 Rust `tags.rs` 的 `ok_prefix` 一致。
  const re = new RegExp(`(?:^|\\s)${prefix}([${chars}]{1,64})`, "gu");
  const out: string[] = [];
  const seen = new Set<string>();
  for (const match of body.matchAll(re)) {
    const raw = match[1]!;
    // 纯数字不是标签(`#42` 是条目编号),同 Rust `normalize_tag`。
    if (prefix === "#" && /^\d+$/.test(raw)) continue;
    const token = prefix === "#" ? raw.replace(/[/-]+$/, "") : raw;
    if (!token) continue;
    const key = token.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(token);
  }
  return out;
}

/** 合并两个 token 列表,大小写不敏感去重,前者优先。 */
function mergeTokens(primary: string[], extra: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const token of [...primary, ...extra]) {
    const key = token.toLowerCase();
    if (!key || seen.has(key)) continue;
    seen.add(key);
    out.push(token);
  }
  return out;
}

/** 候选池:还没打分、没截断的全量。 */
function poolFor(source: CompletionSource): Omit<CompletionItem, "spans">[] {
  if (source.kind === "emoji") {
    return EMOJIS.map((entry) => ({
      id: entry.code,
      label: `:${entry.code}:`,
      detail: entry.words.join(" · "),
      glyph: entry.emoji,
      // 后面补一个空格:emoji 紧贴着后文时中英混排会挤在一起。
      insert: `${entry.emoji} `,
    }));
  }

  if (source.kind === "tag") {
    /* 全库扫描的结果放前面。它是"整个库里确实存在的标签",比只在当前笔记里出现过的
       更可能是用户想要的那一个;而当前笔记里刚写的那些排后面也照样能搜到。
       用 `normalizeTag` 去重是为了和标签云一致 —— 那边把 `#Work` 和 `#work` 折成
       一条,补全里列出两条会让人以为是两个标签。 */
    const merged = mergeTokens(
      source.vaultTags.map((tag) => normalizeTag(tag) || tag),
      tokensInBody(source.body, "#"),
    );
    return merged.map((tag) => ({
      id: `tag:${tag.toLowerCase()}`,
      label: `#${tag}`,
      glyph: "#",
      insert: `#${tag} `,
    }));
  }

  if (source.kind === "mention") {
    return tokensInBody(source.body, "@").map((token) => ({
      id: `mention:${token.toLowerCase()}`,
      label: `@${token}`,
      glyph: "@",
      insert: `@${token} `,
    }));
  }

  // wiki:全部笔记。链接目标用标题而不是路径 —— `resolveLink` 的 byStem → byTitle
  // 两条都能解析到,而标题是用户认得的那个。
  return source.notes.map((note) => ({
    id: `note:${note.id}`,
    label: note.title,
    detail: note.id.slice(note.id.lastIndexOf("/") + 1),
    glyph: "📄",
    insert: `[[${note.title}]] `,
  }));
}

/**
 * 算出一次菜单要显示的候选。
 *
 * 空查询:按池子原序给前 `COMPLETION_LIMIT` 条(标签是"全库的在前",笔记是路径序,
 * emoji 是表序)。非空:按 `scoreFuzzyMatch` 降序,同分按原序 —— 稳定排序保证同一个
 * 查询两次得到同一个第一条。
 */
export function buildCompletions(source: CompletionSource): CompletionItem[] {
  const pool = poolFor(source);
  if (source.query === "") {
    return pool.slice(0, COMPLETION_LIMIT).map((item) => ({ ...item, spans: [] }));
  }

  const scored: {
    item: Omit<CompletionItem, "spans">;
    score: number;
    spans: CompletionItem["spans"];
  }[] = [];
  for (const item of pool) {
    /* 只拿 label 和 detail 里分高的那一个的命中区间。两个都画的话,`detail` 命中时
       会在 `label` 上画出一段位置对不上的高亮(命中区间是按另一个字符串算的)。 */
    const onLabel = scoreFuzzyMatch(source.query, item.label);
    const onDetail = item.detail ? scoreFuzzyMatch(source.query, item.detail) : null;
    if (!onLabel && !onDetail) continue;
    // label 命中优先:那是用户眼睛真正在看的那一行,detail 只是消歧用。
    const best = onLabel ?? onDetail!;
    scored.push({ item, score: best.score, spans: onLabel ? onLabel.spans : [] });
  }
  /* 同分保持池子原序 —— 这里**不**额外写一个下标比较器:`scored` 是按 pool 顺序 push
     的,而 `Array.prototype.sort` 自 ES2019 起规定是稳定排序,同分项的相对顺序本就不
     会变。加一个只是把同一件事写两遍,而且去掉它任何测试都发现不了。 */
  scored.sort((a, b) => b.score - a.score);
  return scored.slice(0, COMPLETION_LIMIT).map(({ item, spans }) => ({ ...item, spans }));
}
