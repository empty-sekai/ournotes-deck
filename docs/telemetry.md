# 搜索埋点（`ournotes-deck.telemetry/1`）

[English](telemetry.en.md)

推荐结果（`ournotes-deck.recommendation-result/3`）和搜索会话进度（`ournotes-deck.search-session-progress/2`）都带一份
`telemetry`。两处结构相同：会话进度给的是到当前为止的工作，推荐结果给的是整次请求。

## 约定

- **以 `Ms` 结尾的键都是毫秒**：实测墙钟时间，或请求里的时限。去掉这些键后，余下内容只取决于输入和停止位置：
  同一输入的完成搜索每次都相同，被同一候选数上限停下的搜索也相同。
- **收益分子**都是十进制字符串，分母是 `environment.target.denominator`（实打 Live 为 120：对所有出场顺序求和），
  与 `results[].expectedPayoff` 一致。
- **深度**：联合搜索按队长优先依次放成员，深度 `d` 表示已放 `d` 名成员（0 是根，5 是完整队伍）。
  长度为 6 的数组按深度下标。
- 下表“用途”一栏：**页面** 表示适合展示给玩家，**开发** 表示用于定位性能和正确性问题。

## 顶层

| 字段 | 含义 | 用途 |
|---|---|---|
| `format` | `ournotes-deck.telemetry/1` | — |
| `environment` | 程序、数据、请求预算和上界配置 | 页面（数据身份）、开发 |
| `proof` | 是否已证明、进度、未探索部分的上界与差距 | 页面 |
| `incumbents` | Top-K 随时间的变化 | 页面 |
| `phases` | 按时间顺序的阶段 | 页面（简化）、开发 |
| `time` | 搜索循环内各活动的独占时间 | 开发 |
| `nodes` | 所有遍历的搜索树节点数 | 页面、开发 |
| `leaves` | 队伍按出场顺序的评估、模拟与截断 | 开发 |
| `joint` | 联合搜索每层的节点与上界检查 | 开发 |
| `composition` | 成员组合遍历的计数 | 开发 |
| `candidate` | 启发式候选策略的计数 | 开发 |
| `caches` | 各缓存的查询与命中 | 开发 |
| `memory` | 程序的峰值内存 | 页面、开发 |
| `lotteryRefinement` | LUCK 完整 nominal 路径细化的工作量 | 开发 |

## `environment`

| 字段 | 含义 |
|---|---|
| `crateVersion` | 库版本 |
| `commit` | 构建时环境变量 `OURNOTES_DECK_COMMIT` 的值；未声明时为 null |
| `features` | 启用的特性（如 `search-diagnostics`） |
| `arch`、`os` | 目标架构与系统（WASM 为 `wasm32`） |
| `optimized` | 是否为无调试断言的构建 |
| `data` | 数据身份：`region`、`masterVersion`、`clientVersion`、`resourceVersion` 取自数据文件的 provenance（缺失为 null），`sha256` 是数据文件 JSON 文本的 SHA-256 |
| `route` | 求解路线：`canonicalPowerSkip`、`physicalExhaustive`、`physicalBranchAndBound`、`physicalCandidate` |
| `traversal` | 实际遍历：`none`、`canonical`、`fixed`、`joint`、`composition`、`exhaustive`、`candidate`、`session` |
| `k` | Top-K 的 K |
| `timeLimitMs`、`maxCandidates`、`cacheEntries` | 搜索开始时的剩余时限（已扣除建池时间）、候选数上限、去重缓存容量 |
| `target` | 实打 Live：`orders`（120 种出场顺序，等概率）与 `denominator`（收益分母）；综合力与 Skip 为 null |
| `domain` | 合法域：`members`、`snaps`、`required`（必须成员数）、`leaderFixed` |
| `bounds.compiled`、`bounds.fallback`、`bounds.compileMs` | 是否编译了联合上界、回退完整枚举的原因、编译耗时 |
| `bounds.choices` | 联合搜索每层的分支数（成员与 Snap 选择的组合数） |
| `bounds.correlated`、`bounds.resource` | 相关上界、资源上界是否启用（最后一个搜索分段） |
| `bounds.fine`、`bounds.classSearch` | 有精细上界、使用分类搜索 |
| `bounds.ptRegime` | PT 预热后的收紧：`membersRemoved` 去掉的成员数、`fallback`、`compileMs`；没有走这一步时为 null |
| `bounds.conversion` | 撃奏分数按转换 Snap 分段：`snaps` 转换 Snap 数、`parts` 分段数、`fallback`、`compileMs`；不分段时为 null |

## `proof`

| 字段 | 含义 |
|---|---|
| `complete` | 搜索是否穷尽（结果即已证明的 Top-K） |
| `fraction` | 按位置估计的已决定比例，0 到 1；完成时为 1。遍历当前路径之前的分支都已评估、剪枝或跳过。各分支大小不同，这是进度指示，不是剩余时间估计。没有位置跟踪的遍历为 null |
| `parts`、`partsDone` | 依次搜索的分段数与已完成数（撃奏分数的转换分段；其他为 1） |
| `topLevelDone`、`topLevelTotal` | 当前分段里已决定的顶层分支数与总数（联合搜索的深度 0 选择，或成员组合遍历的队长）；未完成时给出 |
| `best`、`kth` | 当前最佳与第 K 名的精确收益分子；Top-K 为空时 `best` 为 null，未满时 `kth` 为 null。LUCK 区间路径两者均为 null，收益范围见结果的 `payoffInterval` |
| `upperBound` | 停止时**尚未探索部分**的收益上界（分子） |
| `globalUpperBound` | 全域最佳收益分子的上界，覆盖已找到的编成与全部未关闭分支；得到后只降不升。确定性 physical 搜索完成时等于 `best`，也适用于完整枚举及多个转换分段。已完成的空域、LUCK 区间路径及不跟踪此字段的遍历为 null |
| `bestGap`、`kthGap` | `(upperBound − x) / x`，上界不超过 x 时为 0；x 不为正时为 null |
| `boundMs` | 停止后计算 `upperBound` 的耗时（不占搜索时限） |

确定性搜索的 `upperBound` 是真上界：全域最佳编成的收益不超过 `max(best, upperBound)`；真正 Top-K 里没有被保留的编成，
收益都不超过 `max(upperBound, kth)`。因此超时时可以显示“未证明，最优与当前最佳相差不超过 `bestGap`”。

计算方式：停止后，沿当前路径对每一层剩余的分支，取搜索本身在那里会检查的上界（节点上界、尾部上界；
根层逐个分支取深度 1 的节点上界，根层按这个上界降序访问时就是剩余分支里的第一个；尚未开始的转换分段取全池上界），再取最大值。PT 预热阶段就停止时，
预热按奖金过滤跳过的前缀也算未探索，取全域根上界。

运行中的某一转换分段不能单独提供全域上界，`globalUpperBound` 此时可能为 null。停止时将已保留的最佳值与覆盖所有剩余分段的
`upperBound` 合并，就能提供全域上界；完成时所有分段均已关闭。LUCK 的已评估候选在区间前沿中，不能将它们当作空的精确 Top-K 或零收益。

`upperBound` 为 null 的含义：已完成时没有未探索部分；`traversal` 为 `exhaustive`、`candidate`、`canonical`、`session` 时不记录未探索上界
（`bestGap` 也为 null）；有上界的遍历停止时若已没有未探索的编成，`upperBound` 为 null 而差距为 0。
进度报告取自搜索进行中，不是停止：`complete` 为 false，`upperBound` 和差距为 null。

## `incumbents`

| 字段 | 含义 |
|---|---|
| `updates` | Top-K 插入次数 |
| `stride` | 时间线记录每第 `stride` 次插入；满 256 条时隔一条删一条并加倍。最后一次插入总会记录 |
| `timeline[]` | `update` 序号、`atMs`（距请求开始）、当时的 `nodes`、`candidates`、`simulations`、`filled`（Top-K 已有几副）、`best`、`kth`、`fraction`、`upper`（当时的 `proof.globalUpperBound`，尚未得到时为 null） |
| `firstFull` | Top-K 第一次放满时的那一项（字段同 `timeline[]`；时间线抽稀后可能不含它）；没放满时为 null |
| `warmStart.evaluations`、`warmStart.leafBoundChecks` | 联合搜索遍历前的 warm start（阶段 `seed`）精确评估的编成数，以及它的局部搜索和打磨计算叶子上界的次数 |
| `warmStart.kth` | warm start 结束时第 K 名的收益分子；Top-K 没放满时为 null |
| `warmStart.finalTopK` | 最终 Top-K 里由 warm start 或打磨先评估到的编成数 |
| `warmStart.polishRounds`、`warmStart.polishEvaluations`、`warmStart.polishMs` | 遍历中找到严格更好的最佳编成后，在它附近打磨的轮数、精确评估数和耗时（计入搜索阶段） |

warm start 和打磨只把精确评估过的合法编成放进 Top-K，不剪任何分支；遍历再遇到它们时按已评估处理。

页面可用 `atMs` 对 `best`／`kth` 画“随时间收敛”的曲线。

## `phases`

按时间顺序、互不重叠，阶段之间的少量空隙不属于任何阶段。每项：`name`、`label`、`startMs`（距请求开始）、
`wallMs`，以及该阶段内增加的 `nodes`、`candidates`、`simulations`。

| `name` | 含义 |
|---|---|
| `prepare` | 解析场景、培养与目标，建立合法域（不含上界编译）；对已建好的问题执行时没有 |
| `boundCompile` | 编译联合上界 |
| `initial` | 评估请求给出的 `initialDecks` |
| `setup` | 相关上界与资源上界的启用判断、warm start 准备 |
| `ptWarmStart` | PT：先在最高奖金区间找出 Top-K |
| `ptRegimeCompile` | PT：按当前第 K 名去掉不可能入选的成员并重新编译上界 |
| `conversionCompile` | 撃奏分数：编译各转换分段的上界 |
| `seed` | 联合搜索遍历前的 warm start（见 `incumbents.warmStart`） |
| `search` | 主搜索；转换分段时每个分段一项，`label` 为 `free`、`snap <Snap ID> slot <槽位>` 或 `pair slots <i>,<j>` |
| `evaluate` | 评估指定编成 |
| `warmStart`、`proposals` | 启发式候选策略的两段 |
| `verify` | Power/Skip 规范搜索的结果复核 |
| `finish` | 整理结果 |

## `time`

从开始求解到结束，按活动划分的独占时间（毫秒），各项之和等于这段时间：

| 字段 | 含义 |
|---|---|
| `depthMs[d]` | 联合搜索深度 `d` 的节点工作：上界、分支循环和簿记，不含下面各项 |
| `compositionMs` | 成员组合遍历的节点工作 |
| `fineBoundMs` | 完整队伍按出场顺序的原始上界与精细上界 |
| `cutoffTableMs` | 构建模拟截断表 |
| `simulationMs` | 跑完的整局模拟 |
| `stoppedSimulationMs` | 中途截断的整局模拟 |
| `warmStartMs` | warm start 与打磨，不含其中的模拟和截断表 |
| `otherMs` | 其余：准备、编译、整理结果、其他遍历，以及停止后计算 `proof.upperBound` 的时间（`proof.boundMs`） |

## `leaves`

| 字段 | 含义 |
|---|---|
| `proposed` | 交给评估的候选编成次数（含命中去重缓存的，见 `caches.candidates`） |
| `visited` | 新候选数（`maxCandidates` 按它计）；实打 Live 是按规范站位的队伍 |
| `evaluated` | 所有出场顺序都算完的候选数 |
| `partial` | 被停止打断评估的候选数 |
| `cheapPruned`、`finePruned` | 一局都没模拟就放弃的队伍：各出场顺序的廉价上界之和，或原始与精细上界之和，低于第 K 名 |
| `started` | 开始逐个出场顺序模拟的队伍数 |
| `orderBoundPruned` | 部分出场顺序算完后，精确收益加上其余顺序的上界仍低于第 K 名而放弃的队伍数 |
| `simulations` | 跑完的整局模拟次数（每次一个出场顺序） |
| `cutoff.tables` | 带截断表模拟的出场顺序数 |
| `cutoff.unavailable` | 没有有限截断表的出场顺序数 |
| `cutoff.stopped` | 中途截断的模拟次数（该队伍不可能进入 Top-K） |
| `cutoff.stoppedAt[i]` | 截断发生时已播放帧占全谱的比例落在 `[i/10, (i+1)/10)` 的次数 |
| `peakRetained` | Top-K 保留的最多编成数 |

## `joint`

联合搜索（撃奏）每层的计数，数组按深度下标。节点上界记在被检查节点的深度；`tail`、`pair` 记在父节点的深度
（检查的是它的子分支）。每类上界是 `{checks: [6], pruned: [6]}`。

| 字段 | 含义 |
|---|---|
| `nodes` | 各深度节点数 |
| `branch` | 基本节点上界 |
| `assignment` | 收益与第 K 名相同时的综合力分配上界 |
| `correlated`、`resource` | 相关上界、资源上界 |
| `bonus`、`bonusUnavailable` | PT 奖金上界及其不可用次数 |
| `tail`、`tailChoicesSkipped` | 一次排除一段剩余分支的尾部上界，及因此跳过的分支数 |
| `pair` | 单个子分支的上界 |
| `nodeTies`、`pairTies` | 上界等于第 K 名、靠综合力保留的次数 |
| `seedBonusSkipped` | PT 预热时不在最高奖金区间而跳过的前缀 |
| `modules` | 按名称的上界模块（`memberAdditive` 等；`carrierSplit`：按空位将放的连击载体拆分的节点上界，载体见 `carriers`；`carrierSplitTail`：同一上界用于节点选择循环里剩下的全部子节点），各为全部深度合计的 `{checks, pruned}` |
| `carriers` | 撃奏分数且谱面有连击区间时，按连击载体数分档的廉价上界（载体：带撃奏连击加成窗口的成员与 Snap；已放 `c` 个载体、还剩 `r` 个空位的节点读第 `c + r` 档）：`levels` 是全池上界之外编译的档数（各搜索分段取最大），`nodes[n]` 是读第 `n` 档（0 到 5）的受检节点数 |
| `rootOrder` | 根层按深度 1 上界降序访问：`skipped` 是某个分支已严格劣于第 K 名后不再访问的根分支数（它们都会在深度 1 被剪），`traversalsPruned` 是最好的根分支一开始就已劣于第 K 名的遍历数（整个域或撃奏的一个转换分段及其槽位规则） |

## `composition`

普通 Live 的成员组合 → Snap 遍历：`memberNodes`（按已放成员数）、`snapNodes`、`classNodes`、
`bindingNodes`、`compositions`（到达的成员集合数）；上界 `composition`、`team`（整个成员组合及其部分 Snap 配对）、
`class`、`classBinding` 各为 `{checks, pruned}`；`modules` 为按名称的上界模块（`memberAdditive`：无撃奏 Live 的分数），
各为 `{checks, pruned}`；`classInfeasible`、`classResourceChecks`、`classResourceTightened`；
各类种子候选 `seeds.{preseed, team, weighted, class, powerFrontier}`；`powerFrontierClosed`。

## `candidate`

启发式候选策略：`warmupMemberSets`、`warmupProposals`、`explorationProposals`。

## `lotteryRefinement`

物理候选域已穷尽后，仅细化仍影响排序的候选与顺序。所有计数是本次请求的累计值；工作配额耗尽或拒绝细化不构成证明。

| 字段 | 含义 |
|---|---|
| `attemptedOrders` | 尝试构造完整 nominal law 的顺序数 |
| `completedOrders` | 取得完整且总质量精确等于 1 的概率律的顺序数，包括初始模型相等时复用的完整概率律 |
| `installedOrders` | 完整 law 成功用于收紧排序前沿的顺序数 |
| `declinedOrders` | 因不支持的输入、随机源、工作配额、取消或算术容量而未得到完整 law 的顺序数 |
| `arithmeticDeclines` | law 已完整，但搜索侧精确收益算术无法表示，因此未安装的顺序数 |
| `replayRuns`、`frames`、`terminalPaths` | 所有细化尝试中已启动的重放段数、已执行帧数、已完成终止路径数；复用完整概率律不增加重放工作量 |

细化运行计入 `time.simulationMs`，与原有 `leaves.simulations` 的每队 120 顺序粗求值分别计数。排名得到认证时可以停止，
因此 `Complete` 不要求每个顺序都完成精确细化，也不保证结果中已有精确的有理数期望值。

## `caches`

`candidates`（已评估或剪掉的编成；实打 Live 为按规范站位的队伍）、`bonusRows`（PT 奖金上界的行表）、`rushWindows`
（Rush 区间窗口）各为 `{lookups, hits, evictions, peakEntries}`；`evictions` 是丢弃的条目数（整表清空时计全部条目）。
`bonusRowsRefused` 是因容量上限而放弃该上界的次数。

## `memory`

`peakBytes`：写出这份文档时程序占用过的最大内存。WebAssembly 下是线性内存的大小（`memory.buffer.byteLength`），
它只增不减，所以覆盖实例的整个生命周期；Linux 下是进程的峰值常驻内存；其他平台为 null。

## 页面展示建议

- 状态：`proof.complete`；未完成且 `proof.bestGap` 不为 null 时显示“未证明，最优与当前最佳相差不超过 x%”。
  `proof.globalUpperBound` 与 `proof.best` 对比给出全域的同一结论。
- 进度：`proof.fraction`（注明是按位置估计），或 `proof.topLevelDone / proof.topLevelTotal`。
- 收敛曲线：`incumbents.timeline` 的 `atMs` 对 `best`、`kth`、`upper`。
- 耗时构成：`phases` 的 `name`、`startMs`、`wallMs`。
- 数据身份：`environment.data`（区服、master 版本、数据 SHA-256）。
- 规模：`nodes`、`leaves.visited`、`leaves.simulations`。

其余字段面向开发定位。
