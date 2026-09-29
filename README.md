# ournotes-deck

BanG Dream! Our Notes 的综合力、跳过分数与演出分数计算，以及精确的 Top-K 组卡搜索。

[English](README.en.md)

## 计算内容

- **综合力**：逐槽位各项（等级、突破次数与卡面等级带来的卡面属性；角色等级与总角色等级；乐队道具；歌曲属性与标签加成；
  snap 的加成与属性连携；队长技能；VIP；回忆；活动参数）与编成总值，按游戏自身的整数与 binary32 浮点运算，包括其舍入与取整方式。
- **跳过分数**：某张谱面跳过演出时的分数。
- **演出分数**：单个音符分数、帧、连击加成表、倍率指令，以及逐帧的整场演出（`live::full`）：判定转换、连击、血量（回复、护盾、
  归零）、分数计算器（包括迟到判定引起的回退重算），以及演出技能与 snap 技能的条件和效果。撃奏（区间状态、撃奏连击与 Just 计数、幸运抽签、排名加成，以及撃奏技能与撃奏 snap 技能）也已建模；搜索目前只支持撃奏关闭。
- **活动点数**：活动加成、评级、加成道具倍率，以及游戏客户端计算的活动点数。实际发放量由游戏服务器决定；本库复现的是客户端自身的计算。
- **搜索**：综合力（可带或不带歌曲、可带或不带活动参数）、跳过分数与演出分数的前 K 个编成，每组 5 张成员卡只保留一个结果。
  演出分数是判定序列下含演出技能与 snap 技能的整场模拟（撃奏关闭）；排除 snap 技能时，则是逐音符打法下只含演出技能的分数。
  含 snap 技能时搜索要模拟候选编成，因此较慢；漏判或迟判较多的判定序列可能慢得多，可用时间上限约束这类请求。

## 正确性

正确性分三层，各层证明的内容不同。

**与游戏一致。** 各项计算按游戏客户端的代码逐函数实现，并逐个计算单元与客户端自身的实现对照：在模拟器中以相同输入执行
客户端的 arm64 原生函数，按位比较结果（浮点按位比较，整数溢出与抛出异常的路径也一并比较）。对照覆盖：
- 综合力的数值原语与槽位计算；
- 音符分数与帧、连击加成；
- 倍率指令与分数计算器的回退重算；
- 演出技能与 snap 技能的条件和效果；
- 血量、连击与判定；
- 随机数；
- 评级与活动点数。

生成输入共数百万组，包括真实 master 行，差异为 0。对照时还运行了故意改错的版本，以确认比较确实能发现差异。编成五槽求和与
各项加成的构建是纯整数代码，没有单独执行，按代码逐函数移植。本库的实现再与这套经过对照的模型比较，差异同样为 0，其中整场
逐帧演出在撃奏关闭与开启时各比较了 5 万多个场景，包括 1,048 与 1,267 首真实谱面整首。

**搜索精确。** `Complete` 的搜索结果恰为全部合法编成上的规范 Top-K。剪枝只使用在游戏运算下已证明可采纳的上界（证明见
[docs/search.md](docs/search.md)）。搜索结果与独立的穷举实现逐项比较，穷举不共用任何上界、分解或 Top-K 代码：在真实卡牌与谱面上
比较了 25,600 组请求、共约 4.5 亿个编成，差异为 0。
含 snap 技能的演出分数，穷举会模拟每一组成员、队长、snap 配置与演出顺序：在真实卡牌与谱面上（默认与随机判定序列）比较了
1,280 组请求、共模拟约 220 万个编成与顺序，在 snap 技能会改变排名的合成卡池上比较了 3,200 组请求、共约 1.09 亿个，差异均为 0。
达到时间上限的搜索返回 `TimedOut`，其中的编成合法且数值精确，但不保证排名。
超出已证明范围的输入、未知卡牌、游戏会拒绝的规则以及尚未建模的部分，都以错误返回。

**尚未验证的部分。** 整场演出如何由各单元按帧拼成，没有与游戏整体对照：这需要一份真机录制的对局（随机种子、帧时间与逐音符
判定）。撃奏开启时分数取决于随机种子；搜索尚未提供撃奏目标。

## 数据

本库不含任何游戏数据。它读取一份 deck data 文件（`nnnotes.deck-data/1`，由 `nnnotes deck-data` 命令生成，含同一版本
master 数据的相关表与全部谱面），以及用户的卡牌持有情况：

```json
{
  "player": { "characterRanks": { "1": 20 }, "bandItems": { "101": 10 }, "vipRank": 3, "events": [] },
  "members": [ { "id": 1, "level": 60, "awake": 3, "rank": 2, "liveSkillLevel": 3, "gekisouSkillLevel": 1 } ],
  "snaps": [ { "id": 1, "level": 20, "rank": 1 } ]
}
```

`level` 可以换成 `exp`。谱面按 score id（`MasterLiveMusicScore._id`）选择。

含 snap 技能的演出分数按判定序列模拟：
`{"frames": [0, 16, 33], "judged": [[frame, noteId, judgement, judgementTimeMs]], "baseSeed": 0, "assist": false}`。
`frames` 是每一帧的乐曲时间（毫秒，不减）；`judged` 的每一行表示某个音符在第 `frames[frame]` 帧判定，判定为转换前的判定
（1 Miss、2 Bad、3 Good、4 Great、5 Perfect、6 Just）；血量、连击与技能由模拟得出。默认判定序列为理论最佳：以 60 fps
取帧（`floor(i * 1000 / 60)` 毫秒），直到最后一个判定音符或技能事件之后 2000 毫秒；每个判定音符在第一个到达其谱面时间的帧
判为 Perfect，判定时间取谱面时间（撃奏关闭时游戏不判 Just）。

排除 snap 技能的演出分数默认使用逐音符形式的理论最佳打法：每个判定音符都是 Perfect，全连且不掉血。也可以另给一份打法
`{"notes": [{"noteId", "timeMs", "noteType", "scoreType", "life", "combo"}], "lifeAtEvent": [...], "assist"}`
（判定类型：1 Just、2 Perfect、3 Great、4 Good、5 Bad、6 Miss）。

## 作为库使用

```rust
use ournotes_deck::cards::Roster;
use ournotes_deck::data::DeckData;
use ournotes_deck::search::{Constraints, Objective, Pool, SearchRequest, search};

let data = DeckData::from_path("deck-data.json").unwrap();
let roster = Roster::from_json(&std::fs::read_to_string("box.json").unwrap()).unwrap();
let pool = Pool::new(&data.master, &roster).unwrap();
let out = search(&pool, &SearchRequest {
    objective: Objective::SkipScore { score_id: 10000103, chart: data.chart(10000103).unwrap() },
    k: 10,
    constraints: Constraints::default(),
    time_limit: None,
})
.unwrap();
for deck in &out.results {
    println!("{:?} {} {:?} {:?}", deck.score, deck.power, deck.members, deck.snaps);
}
```

## 命令行

```sh
ournotes-deck power --data deck-data.json --roster box.json -k 10 [--music ID] [--event]
ournotes-deck skip  --data deck-data.json --roster box.json --score SCORE_ID -k 10
ournotes-deck live  --data deck-data.json --roster box.json --score SCORE_ID [--play stream.json] -k 10
ournotes-deck live  --data deck-data.json --roster box.json --score SCORE_ID --exclude-snap-skills [--play play.json] -k 10
```

`live` 默认计入 snap 技能，`--play` 为判定序列；加 `--exclude-snap-skills` 时只计演出技能，`--play` 为逐音符打法。

约束：`--leader ID`、`--include ID,...`、`--exclude ID,...`、`--exclude-snaps ID,...`、`--no-snaps`、
`--time-limit-ms N`。输出为 JSON。

谱面统计：

```sh
ournotes-deck chart-stats --data deck-data.json [--seeds 8] [--charts ID,...] [--jobs N] -o chart-stats.json
```

在整场模拟上实测每张谱面与卡组无关的量（`ournotes-deck.chart-stats/2`），分两种场景：激走开启（`seeds`，撃奏ライブ
的打法）与激走关闭（`offSeeds`，自由 Live、挑战 Live 等单人 Live 的打法）。`--charts` 只测列出的 score id（按文件中的
顺序输出，文件里没有的 id 忽略）；`--jobs N` 同时测 N 张谱面，输出与逐张测量完全相同。不给 `-o` 时输出到标准输出。

激走开启时打法为激走理论最佳：每个音符按准点判定，Just 任务区间内为 Just，其余为 Perfect，每个区间取名次 1。每个
种子给出：无技能的精确得分；各激走区间的结果（`ranges`：区间得分、名次 1 加成、最大激走连击 `maxCombo`、Just 数
`justCount`、幸运点数 `luckPoints`、抽签结果 `lotResults`；前三个计数分别是连击、Just、幸运任务的名次指标；无技能时幸运点数
只来自幸运区间的抽签，其他区间为 0）；master 中每种加分效果（2000 / 2002 / 2004 / 2005，按类型、时长、目标、
条件分组，见 `kinds`）在每个演出位上因子为 1 时的得分增量除以综合力（`weights[kind][k]`）。卡组得分约为
`P × (score / power + Σ factor_k × weights[kind_k][k])`，每个种子都用 master 真实数值的随机卡组在另一综合力下实跑校验，
偏差超出取整上限即报错。有幸运区间的谱面按前 N 个发布种子给出（`--seeds`，默认 8），这不是原生期望；超过三段 fever
的谱面游戏会在第四段开始时出错，记为 `unplayable`（激走关闭时照常可玩）。

其他名次不重跑：名次加成为 `trunc(区间得分 × 百分比 / 100)`，记在区间结束帧的固定分上，不改因子也不改音符得分，所以
区间 i 取名次 r_i 时无技能得分精确为 `score − Σ rankBonus_i + Σ trunc(rangeScore_i × rankBonusPercents_i[r_i − 1] / 100)`，
权重为 `weights[kind][k] + Σ (rankBonusPercents_i[r_i − 1] − rankBonusPercents_i[0]) / 100 × rangeWeights[kind][k][i]`
（`rangeWeights` 是该效果带来的区间得分增量除以综合力）。每个种子另用同一校验卡组在随机名次下走显式名次确认实跑校验
（`rankCheck`）。条件读取确认名次（7012）的效果种类没有 `rangeWeights`，名次加成可能落进另一区间得分帧的谱面整张没有。
每个种子还给出把 Just 全部改判 Perfect 的同一打法的无技能得分与各区间得分（`scorePerfect`、`rangeScorePerfect`）。

激走关闭时打法为理论最佳（每个音符准点 Perfect），种子 0，没有 Just、幸运、激走连击和名次加成；给出同形的 `score`、
`weights` 与校验。条件读取激走状态的效果种类在激走关闭时无法演出，其权重为 null。


### 激走技能适性

统计默认还给出单技能适性，不选择最佳编成，也不改变上面的 `seeds` / `offSeeds`。文件级 `gekisouAptitude` 是形状表和
测量规则；每谱 `charts[].gekisouAptitude` 是谱面因子 `factors` 与该谱任务对应的变体 `variants`。没有激走区间、不能开
激走或没有可测技能时，每谱字段为 null。格式仍是 `ournotes-deck.chart-stats/2`，这些都是新增字段。

```sh
ournotes-deck chart-stats --data deck-data.json --aptitude-max-seeds 128 --aptitude-cross-seeds 32 -o stats.json
ournotes-deck chart-stats --data deck-data.json --no-gekisou-aptitude -o baseline.json
```

- `--aptitude-max-seeds N`：随机增量最多测 N 个种子，默认 1024，N 至少为 2；标准误足够小时提前停止。
- `--aptitude-cross-seeds N`：普通技能交叉项最多测前 N 个种子，默认 64，N 至少为 1。
- `--no-gekisou-aptitude`：跳过适性测量，文件级和每谱的 `gekisouAptitude` 都为 null，原有统计照常输出。

形状按来源、任务与效果参数去重。成员技能取该技能的最高等级；小卡技能取最高突破对应的等级，不直接取效果表最高等级。
支援技能仅差乐队目标时归为同一形状，保留 `skills[].memberTargetIds` / `bandIds`；每谱分别测 `bandMatch: true/false`。
支援技能的宿主统一使用同任务的**合成空激走技能**，不借真卡，不混入成员技能收益。每次只带一个成员或支援技能，整局实跑。

变体的 `score`、`scorePerfect`、`tail`、区间增量等均为 `[均值, 均值标准误]`，是同种子下「带技能 − 不带技能」的差，
综合力固定为 `model.power`。`tail = Δscore − Σ(ΔrangeScore + ΔrankBonus)` 表示区间外收益，包含技能延续到区间结束后的
尾部；`factors` 给出各区间音符数、进入时连击和基线抽签次数，用来解释适性。`weights` 是普通技能 `plainKind` 各位置权重
的变化，`rangeWeights` 是对应区间权重变化；不是完整编成权重，没有普通 kind 时两者为 null。每个变体的 `check` 用首个
测量种子、随机名次与随机普通技能卡组，在另一综合力下验证线性预测，超出取整界时报错。

随机增量按 32、64、128、256、512、1024 个种子逐级测量（受最大种子参数限制），当 Δscore 的标准误不超过
`max(增量均值绝对值 × 1%, 无技能总分均值 × 0.1%)` 时停止；到上限仍不满足则 `seTargetMet: false`。确定性增量报一个
种子、标准误 0；四个种子恰好相等本身不能证明随机技能是确定性的。种子均值不是游戏的期望，真实种子分布未知，标准误也
不表示模型误差。交叉项可能使用更少种子，见各变体的 `crossSeeds`；无普通 kind 时该值为 0。达到标准误目标可能只是满足基线 0.1%的绝对目标，不代表达到增量 1%的相对精度，也不能据小增量均值的正负断言技能一定有益或有害。

**模型边界：**

- 只影响撃奏ライブ的 `battleLiveScore`，不影响另行上报的 `soloScore`，自由 Live 不加适性收益。
- 只测单技能，**多个技能增量不能相加**：激走连击封顶、幸运槽与 rush 支援交互、Just 数改变相关支援触发等都会破坏可加性。
- 理论最佳打法没有 Great / Miss，12004 连击保护、12006 Great→Perfect、4004 判定窗扩大在此为零；13000 / 13002
  Just 数加成与 11002 幸运点数加成可改变区间指标，但不直接增加得分。任务不符的形状因门控为零，不列进该谱变体。
- 普通技能倍率与名次沿用线性式，名次增量每区间有取整差，由 `check` 验证。Just 率不足 100% 时，仅能给出无普通技能增量的 Just / Perfect 插值估计，
  13005 转换、每 Just 的 2001 支援及 13002 的 Just 数变化不能按比例精确缩放。未测 Perfect 打法的交叉权重，因此普通技能非零时不提供低于 100% Just 的完整适性；Great 比例乘 `1 − 0.2q` 也只是近似。

库调用可用 `chart_stats_with` / `document_with` 与
`Options { seeds, aptitude: Some(AptitudeOptions { max_seeds, cross_seeds }) }`；`aptitude: None` 关闭适性。
不含谱面的 `DeckData` 也可以生成形状表，或直接调用 `aptitude_header(master, kinds, options)`。

## 测试

`cargo test` 运行单元测试，读取合成的 deck data 文件，并在小规模合成卡池上将搜索结果与穷举结果逐项比较。
可用 `OURNOTES_DECK_ORACLE_CASES`、`OURNOTES_DECK_ORACLE_SEED0`、`OURNOTES_DECK_ORACLE_MEMBERS`、
`OURNOTES_DECK_ORACLE_SNAPS` 扩大比较规模。含 snap 技能的演出分数可用 `OURNOTES_DECK_SNAPS_CASES`、
`OURNOTES_DECK_SNAPS_SEED0`、`OURNOTES_DECK_SNAPS_MEMBERS`、`OURNOTES_DECK_SNAPS_SNAPS`、`OURNOTES_DECK_SNAPS_NOTES`、
`OURNOTES_DECK_SNAPS_VARIANTS` 扩大比较规模。

## 许可证

MIT OR Apache-2.0。
