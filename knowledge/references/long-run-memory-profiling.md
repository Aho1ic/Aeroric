# 长跑内存增长的采样口径与静态结论

Aeroric 的典型用法是「开着不关」——多个 agent 任务终端、SSH / shell 多标签、Docker 与笔记面板常驻。这类进程的内存问题不会在功能测试里出现,只在连续跑几小时后表现为「越用越慢」「某个终端画面空白」。

本文记录两件事:**怎么量**(采样口径),和**已经修掉的四处结构性泄漏**(静态分析结论)。参数调优本身不在这里 —— 见文末。

## 为什么需要专门的口径

单看 RSS 只能知道「涨了」,不知道「涨的是什么」。前端资源泄漏和 Rust 侧、WebView 自身的增长混在一条曲线里,分不开。所以要两路并行:

1. **存活计数**(进程内,前端):`src/lib/resourceCensus.ts`。登记「当前还活着几个」而非累计总量 —— 累计的话读数永远在涨,就分不出真泄漏了。
2. **RSS 采样**(进程外):`ps` 轮询。

两路对照才能定位:

| RSS | 存活计数 | 结论 |
| --- | --- | --- |
| 涨 | 涨 | 泄漏在被普查的那几类里,看是哪一类 |
| 涨 | 持平 | 泄漏不在这几类 —— 可能在 Rust 侧、DOM 节点、或普查没覆盖的缓存 |
| 持平 | 涨 | 计数本身记错了(减法没走到),先修仪表 |
| 持平 | 持平 | 这一轮没复现,拉长时间或加大操作量 |

## 存活计数

`resourceCensusSnapshot()` 返回五个数:挂载中的 xterm 实例、持有中的 WebGL context、prose HTML 缓存的条数与字符数、注册中的轮询 timer。

dev 下 `installResourceCensusProbe()` 会把它挂到 `window.__aeroricCensus`,devtools 控制台直接敲:

```js
__aeroricCensus();
// { liveTerminals: 3, liveWebglContexts: 3, proseCacheEntries: 412, proseCacheChars: 1203847, liveTimers: 2 }
```

只在 dev 接线。devtools 本身是 opt-in feature、只在 `tauri dev` 启用(见 `src-tauri/Cargo.toml` 那段注释),所以 release 里既没有控制台也没有这个入口 —— 两道门。

**加减的挂点很重要**:xterm 的计数挂在 `term.onDispose` 上,不是散在各面板的 cleanup 里。后者漏一个就永久偏高,仪表本身变成误报源。WebGL 计数和 prose cache 统计直接复用各自模块已有的计数器,通过 `registerWebglContextProbe` / `registerProseCacheProbe` 注入 —— 不另记一份,避免两份数对不上;注入而非 import 是为了断开循环依赖。

### 判读方式

- **空转基线**:不做任何操作,每 5 分钟读一次,连续两小时。计数持平说明这几类没有自发泄漏。
- **操作前后对照**:开一个面板、读一次;关掉、再读一次。回不到原值就是那条 cleanup 漏了。这是定位单个泄漏点最快的路子,比看曲线有效。
- **撞配额**:`liveWebglContexts` 顶到 6 就不再涨(那是配额上限,不是泄漏)。超出的终端走 DOM 渲染器。

## RSS 采样

进程外轮询,不受前端状态影响:

```bash
# 先拿 pid(dev 下进程名是 aeroric)
pgrep -f 'Aeroric|aeroric' 

# 每 60 秒一行:RSS(KB)、CPU%
while :; do printf '%s ' "$(date +%H:%M:%S)"; ps -o rss=,%cpu= -p <pid>; sleep 60; done | tee rss.log
```

macOS 上 `ps` 的 RSS 单位是 KB。Tauri 应用有多个进程(主进程 + WebView helper),**两个都要采** —— 前端泄漏体现在 helper 上,Rust 侧体现在主进程上,只看一个会看漏。

判读时注意 WebView 的 GC 是惰性的:短期波动几十 MB 是正常的,看两小时的趋势线而不是单点。

## 已修的四处结构性泄漏

这些是静态分析找出来并修掉的,不需要实测就能确认是 bug。列在这里是为了避免回归。

### SSH 会话:前端 dispose 不 kill 后端

`SshTerminalPanel` 的 effect cleanup 原先只调 `runtime.dispose()`(收 xterm),后端那条 ssh 进程还活着。这个 cleanup 在两种情况下跑:面板内换连接、组件卸载 —— 两条路原先都不 kill,于是每切一次就在后端遗弃一个 ssh 进程。多标签会把它放大到标签数倍。

修法是 cleanup 里补 `kill_ssh_shell`。后端那个命令是幂等的(`ssh.rs`),所以和显式断开路径重复调用无害 —— 测试里的 `expectKilledOnly` helper 就是允许重复 kill、只断言目标 shellId 正确。

### WebGL context:超配额时浏览器静默丢弃最老的

浏览器对 WebGL 上下文有全局硬上限(通常 8–16),**超出时不报错**:它静默丢弃最老的那个。症状是「开久了某个终端画面空白」—— 那个终端的 context 被后来者顶掉了,而它自己不知道。

长跑下这个上限很容易撞到:SSH 多标签(最多 10)+ 本地 shell 多标签(最多 10)+ 每个 agent 任务一个终端,全都常挂。

修法是 `terminalShared.ts` 里的模块级配额 `MAX_WEBGL_TERMINALS = 6`,满了就不创建、直接走 DOM 渲染器。取 6 是保守值 —— 同时真正在看的终端不会有那么多,超出的那些慢一点但画面正确。**正确优先于快。**

配额计数器的释放要覆盖三条路:`term.onDispose`、`onContextLoss`、以及构造函数抛异常。漏掉第三条会永久占额 —— 计数已经 +1 但对象没建成,再也没人来减。

### prose HTML 缓存:只限条数不限字节

`SessionView` 的缓存原先只有 3000 条的上限。单条可以很长(几十 KB 的日志块或代码块),3000 条 × 30KB 就是 90MB,而条数上限一次都不会触发。

修法是加第二个上界 `PROSE_HTML_CACHE_MAX_CHARS = 8_000_000`(约 16MB UTF-16),两个上界都要满足才停止淘汰。条数挡住「大量小消息」,字符数挡住「少量大消息」。

记账要同时算 key 和 value:key 里含完整原文,长度和 value 同量级,只算 value 会低估一半。淘汰时留最后一条 —— 刚插进来的那条本身可能就超上界,连它一起淘汰会让本次结果不在缓存里,下次重算,白付一次渲染。

### 面板卸载丢状态(不是内存泄漏,但同一批修的)

Docker 与笔记面板原先是条件渲染,切走就卸载、状态全丢(笔记的编辑器状态、Docker 的容器列表)。改成和 sftp / database 一样的模式:挂载标志 + `display` 切换,常驻不卸载。

代价是常驻内存变高 —— 这是有意的权衡:用户切回来要看到原样。可见性判断集中在 `viewMode.ts` 的 `centerLayerVisibility()`,避免「两层同时可见」这类 bug 散在 JSX 里。

## 还没做的:参数调优

以下参数**没有**基于实测调整过,因为增长曲线要真机跑几小时才看得出来:

- 终端 `scrollback`(当前 1000 行/终端)
- 各处轮询间隔
- 终端休眠策略(不可见的终端是否降频)

这三项的合理值取决于实际曲线的形状 —— 是缓慢线性涨(scrollback 类),还是阶梯式跳(缓存类),还是跟操作次数相关(cleanup 类)。**先用上面的口径跑出数据,再定要不要调、调到多少。** 没有数据就改这些参数,只是换一组同样没有依据的常量。
