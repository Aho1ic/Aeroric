# Agent Note: 参考代码库对 Agent 有没有用（R6-2）：真实问题前后对照，结论是保持默认开启

Status: implemented

## Problem

[参考代码库](../feature/2026-11-13-reference-library.md)第一批上线时，`search_references`、`read_reference` 默认提供给所有会话。第一批没有验证它有没有净收益。原型 xerj 自己也说，对模型已经熟悉的热门库可能没帮助。[Wiki 的同类评测](2026-11-10-wiki-agent-eval.md)（R4-3）得出"没有净收益"，Wiki 工具因此改成默认关闭。参考代码库要过同一道判定。

用户 2026-11-13 说"按推荐继续"，做法沿用 R4-3：

- 真实问题，每题 4 个要点，命中一个记 1 分；
- 同一模型、同一项目，两组各问一遍；
- 判定：命中率不降低，并且 token 或用时降 20% 以上；或者命中率提高。满足才算有净收益；
- 没有净收益，就像 Wiki 一样改成默认关闭，加开关。

## Decision

**判定结果：有净收益，参考代码库工具保持默认开启，代码不改。**

两组都拿到 24/24。R 组总用时少 26.5%，超过 20% 的门槛。

**两组的条件**

- 项目：trainer_v6（YOLO 训练服务）。题目问的是 mmyolo（本机 `同步空间/mmyolo`，提交 8c4d9dc）怎么实现，trainer_v6 想照着做。
- P 组：参考库为空，没有参考工具。提示里直接给出 mmyolo 的绝对路径，Agent 用 shell 去读。
- R 组：P 组跑完后把 mmyolo 加入参考库（610 个文件、454 个定义）。提示里只说"在参考代码库里，名称 mmyolo"，不给路径。
- 两组共同的条件：
  - 都用 Magpie 的 Codex 配置，模型 `clinepass/cline-pass/deepseek-v4.1-flash`，权限 `auto_edit`；
  - 都要求只读，用中文分点回答，给出路径和数值。

**题目与要点**（mmyolo 8c4d9dc，评分只看这些点）

| 题 | 问什么 | 4 个要点 |
|---|---|---|
| Q1 | YOLOv8 标签分配 | `BatchTaskAlignedAssigner`；所在文件 `models/task_modules/assigners/batch_task_aligned_assigner.py`；yolov8_s 配置 `tal_topk=10`（类默认 13 不算）；`alpha=0.5`、`beta=6.0` |
| Q2 | YOLOv8-s 三项损失 | `CrossEntropyLoss`（sigmoid）权重 0.5；`IoULoss` ciou 权重 7.5；`DistributionFocalLoss` 权重 `1.5/4`；除以 4 是因为和 mmdet 的 DFL 实现不同 |
| Q3 | 最后关闭 Mosaic | `close_mosaic_epochs=10`；`mmdet.PipelineSwitchHook`；`switch_epoch = max_epochs - close_mosaic_epochs`；`dynamic_intervals` 同时把验证改成每个 epoch |
| Q4 | EMA | `EMAHook` + `ExpMomentumEMA`；`momentum=0.0001`；动量公式 `(1-m)·exp(-(1+steps)/gamma)+m`；`update_buffers=True` |
| Q5 | 优化器与调度 | `base_lr=0.01`、`momentum=0.937`；`weight_decay=0.0005`；`YOLOv5ParamSchedulerHook` linear，`lr_factor=0.01`；warmup 3 / bias 0.1 / 动量 0.8 |
| Q6 | RTMDet 分配 | `BatchDynamicSoftLabelAssigner`；rtmdet_l `dsl_topk=13`；`soft_center_radius`、`iou_weight` 默认都是 3.0；`QualityFocalLoss`，`beta=2.0` |

## 结果

| 题 | P 用时 | R 用时 | P 输入 token | R 输入 token | P 工具 | R 工具（检索 / 读 / shell） |
|---|---|---|---|---|---|---|
| Q1 | 29 s | 32 s | 164K | 172K | 11 | 4 / 5 / 0 |
| Q2 | 174 s | 48 s | 924K | 144K | 47 | 5 / 4 / 1 |
| Q3 | 340 s | 283 s | 799K | 661K | 39 | 22 / 23 / 7 |
| Q4 | 90 s | 85 s | 312K | 675K | 23 | 24 / 17 / 2 |
| Q5 | 27 s | 44 s | 142K | 241K | 11 | 4 / 7 / 6 |
| Q6 | 34 s | 18 s | 130K | 148K | 10 | 3 / 7 / 0 |
| 合计 | 694 s | 510 s | 2.47M | 2.04M | 141 | 62 / 63 / 16 |

**得分与总量**

- 得分：两组都是 24/24，要点全中。
- 总量（R 组相对 P 组）：用时少 26.5%，输入 token 少 17.4%，输出 token 少 10.3%。

**R 组怎么用的工具**

- 6 题都用了参考工具，没有一次用 shell 进 mmyolo 目录。Wiki 评测里 8 题只有 2 题用了 Wiki。
- R 组的 16 次 shell 分两类：
  - 8 次在 trainer_v6 里看本项目的现状；
  - 8 次（Q3 七次、Q4 一次）在找本机有没有 mmdet、mmengine 源码，用了 `find /`、`pip download mmdet`，又自己 kill 掉。
- 不给路径，R 组也找到了仓库，答得和给路径的 P 组一样好。这正是参考库要解决的事："Agent 不知道这些文件夹在哪"。

**噪声**

- 每题只跑一次，差异主要来自 P 组 Q2 一题：P 组为了解释"除以 4"，翻了本项目里的 ultralytics 副本，用了 924K token。
- 去掉 Q2 再看：R 组用时少 11%，输入 token 反而多 23%。
- 单题方向不一致。R 组按 token 只在 Q2、Q3 占优，按用时在 Q2、Q3、Q4、Q6 占优。
- 所以这次结论是"按事先的规则算有收益"，不是"证明了每题都省"。

**R 组检索多的题目有共同点**

Q3、Q4 问的 `PipelineSwitchHook`、`EMAHook` 都不在 mmyolo 里，来自 mmdet 和 mmengine，本机没装源码。R 组在 Q3、Q4 各检索了 20 多次，在找一个参考库里不存在的定义。P 组在同样的地方用 `mdfind`、pip 缓存去找 mmdet 源码，也花了大量步数。如果以后要优化，可以从这里入手：检索不到时，在结果里提示"可能来自未收录的依赖"，并列出已登记的库。

## 评测过程中修正的问题

- **会话不是超时，是脚本没翻页**：
  - 第一版脚本显示 P、R 两组的 Q3、Q4 都"超时 900 秒"。
  - 实际原因：`session.updates` 每次最多返回 500 条，脚本每次都从 `after=0` 取，永远看不到第 500 条以后的 `turnEnded`。
  - 改为按 `seq` 翻页后，12 个会话都是 `completed`。用时改用 `turnStarted` 到 `turnEnded` 的时间戳计算，不再用脚本轮询的耗时。
  - 宿主行为本身没错，以后写评测脚本要记得翻页。
- **模型换成 deepseek-v4.1-flash**：
  - 原定的 `codex/gpt-6.1-sol` 在 Magpie 上返回 404"Codex is switched off"；
  - 内置 `codex` 配置的 `auto/gpt-6.1-sol` 一直 504。
  - 换模型对两组一视同仁，但和 R4-3 不是同一个模型，两次评测的数字不能直接比。
- **权限用 `auto_edit`，不用 `ask`**：脚本会拒绝所有审批请求。在 `ask` 下 shell 命令全被拒，P 组无法读文件，所以改成 `auto_edit`，靠提示约束只读。

## Alternatives considered

- **像 R4-3 那样用 Computer Use 在界面里开会话**：更接近真实使用。没有采用：用户要求测试时不抢鼠标和前台。这次通过宿主 RPC 用 `task.run` 提交会话，提交格式和带指纹的版本号与 App 一致。界面上看工具调用的样子仍留在待目检清单。
- **每题跑多遍取平均**：能压住噪声。没有采用：沿用 R4-3 的做法，单次结果按事先定的规则判，把噪声写清楚。
- **P 组不给路径，只说"mmyolo"**：更能体现参考库"帮 Agent 找到仓库"的价值。没有采用：那样 P 组多半找不到或要先搜全盘，比的就是"有没有"，而不是"给了路径之后，检索工具比 shell 好不好用"。给路径是对 P 组有利的设置，R 组在这种设置下仍然不输。
- **R 组效果平平就默认关闭**：和 Wiki 保持一致。没有采用：判定规则是事先定的，这次满足条件。而且 R 组每题都在用工具，Wiki 评测里模型几乎不用 Wiki 工具，情况不同。

## Consequences

- 收益：
  - 参考代码库保持默认开启，不加开关，界面和代码都不用改；
  - 证明了 Codex 会按 `initialize` 说明加载并实际使用这两个工具，不给路径也能找到仓库。
- 代价：
  - 只测了一个模型（deepseek-v4.1-flash）、一个热门库（mmyolo）、"问实现细节"一类问题，且每题一次；
  - 去掉最大的那题，token 反而更多。以后换了模型或发现参考工具在拖累，要重测，不能把这次当定论；
  - 题目要的定义在外部依赖里时，检索会空转多次。
- 验证：
  - 两组共 12 个会话，结果和答案保存在审阅机 `/tmp/acp-probe/eval-final.json`（临时目录，不入库）；
  - 题目和要点见上文。
