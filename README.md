# ournotes-deck

BanG Dream! Our Notes 的综合力、跳过分数与演出分数计算，以及精确的 Top-K 组卡搜索。

[English](README.en.md)

## 计算内容

- **综合力**：逐槽位各项（等级、突破次数与卡面等级带来的卡面属性；角色等级与总角色等级；乐队道具；歌曲属性与标签加成；
  snap 的加成与属性连携；队长技能；VIP；回忆；活动参数）与编成总值，按游戏自身的整数与 binary32 浮点运算，包括其舍入与取整方式。
- **跳过分数**：某张谱面跳过演出时的分数。
- **演出分数**（部分）：单个音符分数、帧、连击加成表、演出技能的倍率指令，以及一条判定序列的最终分数。snap 技能与撃奏尚未建模，
  请求时报告为不支持。
- **活动点数**：活动加成、评级、加成道具倍率，以及游戏客户端计算的活动点数。实际发放量由游戏服务器决定；本库复现的是客户端自身的计算。
- **搜索**：综合力（可带或不带歌曲、可带或不带活动参数）、跳过分数，以及仅含演出技能的演出分数的前 K 个编成，每组 5 张成员卡只保留一个结果。

## 精确性

`Complete` 的搜索结果恰为全部合法编成上的规范 Top-K；剪枝只使用在游戏运算下已证明可采纳的上界（见
[docs/search.md](docs/search.md)）。达到时间上限的搜索返回 `TimedOut`，其中的编成合法且数值精确，但不保证排名。
超出已证明范围的输入、未知卡牌、游戏会拒绝的规则以及尚未建模的部分，都以错误返回。

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

`level` 可以换成 `exp`。谱面按 score id（`MasterLiveMusicScore._id`）选择。演出分数默认使用理论最佳打法：每个判定音符都是
Perfect（撃奏关闭时游戏不判 Just），全连且不掉血。也可以另给一条判定序列
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
ournotes-deck live  --data deck-data.json --roster box.json --score SCORE_ID --exclude-snap-skills [--play play.json] -k 10
```

约束：`--leader ID`、`--include ID,...`、`--exclude ID,...`、`--exclude-snaps ID,...`、`--no-snaps`、
`--time-limit-ms N`。输出为 JSON。

## 测试

`cargo test` 运行单元测试，读取合成的 deck data 文件，并在小规模合成卡池上将搜索结果与穷举结果逐项比较。
可用 `OURNOTES_DECK_ORACLE_CASES`、`OURNOTES_DECK_ORACLE_SEED0`、`OURNOTES_DECK_ORACLE_MEMBERS`、
`OURNOTES_DECK_ORACLE_SNAPS` 扩大比较规模。

## 许可证

MIT OR Apache-2.0。
