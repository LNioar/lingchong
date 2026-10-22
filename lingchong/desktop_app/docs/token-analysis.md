# LLM Token 消耗分析与预测

## 一、LLM 调用全景

DeskPet 有三类 LLM 调用，均通过 `OpenAiCompat`（OpenAI 兼容接口）：

| # | 调用点 | 类型 | 触发时机 | 是否缓存/持久化 |
|---|---|---|---|---|
| 1 | 采集入库 embedding | embeddings API | 每平台每次采集 | 入库后不再重算 |
| 2 | 兴趣向量补算 | embeddings API | agent 周期中兴趣缺向量时 | 落库后不再重算 |
| 3 | 口播文案生成 | chat completions API | agent 周期中分数过阈值时 | 不缓存，每次调用 |

## 二、逐类 token 估算

### 1. 采集 embedding（消耗大头）

**调用路径**：[manager.rs `batch_embeddings`](file:///workspace/deskpet/crates/deskpet-collector/src/manager.rs#L199-L219)

每次采集一个平台，对该批所有热点标题**批量**调一次 embedding：

```rust
let titles: Vec<String> = items.iter().map(|i| i.title.clone()).collect();
embedder.embed(&titles).await
```

**单条标题 token 估算**：中文热榜标题 10-30 字，按 1 字 ≈ 1.5 tokens，取 **30 tokens/条**。

**每批条目数**：B 站/知乎热榜 20-50 条，微博 50 条，抖音/小红书/RSSHub 20-30 条，取平均 **25 条/批**。

**单次采集 embedding token** = 25 × 30 = **750 tokens**

**每日采集次数**（默认配置间隔）：

| 平台 | 间隔 | 次数/天 |
|---|---|---|
| zhihu | 10 min | 144 |
| bilibili | 15 min | 96 |
| weibo | 20 min | 72 |
| rsshub | 30 min | 48 |
| douyin | 30 min | 48 |
| xiaohongshu | 30 min | 48 |
| **合计** | | **456** |

**每日 embedding token** = 456 × 750 = **≈ 342,000 tokens**

### 2. 兴趣向量补算（一次性，可忽略）

**调用路径**：[orchestrator.rs `ensure_interest_embeddings`](file:///workspace/deskpet/crates/deskpet-agent/src/orchestrator.rs#L104-L132)

仅对 `topics_without_embedding()` 的主题调用，结果持久化到 `Interest.embedding`，**只算一次**。

兴趣主题通常 2-4 字（如 "AI"、"数码"），约 3-5 tokens。即使积累 50 个兴趣，也仅 50 × 4 = **200 tokens**，且只发生一次。

### 3. 口播文案生成

**调用路径**：[orchestrator.rs `compose_script`](file:///workspace/deskpet/crates/deskpet-agent/src/orchestrator.rs#L135-L183)

仅在 agent 周期中**最高分过阈值 0.6** 时才调用。

**输入 token**：

| 部分 | 内容 | tokens |
|---|---|---|
| system prompt | 角色设定 + 主人画像 | ~180 |
| user message | 标题 + 平台 + 链接 | ~50 |
| tools schema | 2 个工具定义 | ~150 |
| **合计（无 function call）** | | **~380** |
| 触发 function call 时追加 | tool result JSON | ~100 |

**输出 token**：1-2 句口播文案，**~60 tokens**。

**调用频率**：
- agent cycle 间隔 = `min_announce_interval_min`（默认 45 min）→ 32 次/天
- 过阈值概率：新用户画像空时 ~10%，老用户画像丰富后 ~40%，取均值 **25%**
- 触发 function call 概率约 **30%**

**单次平均 token** = 380 + 60 + 0.3 × (100 + 60) = **488 tokens**

**每日 chat token** = 32 × 25% × 488 = **≈ 3,900 tokens**

## 三、每日总量预测

| 类别 | tokens/天 | 占比 |
|---|---|---|
| 采集 embedding | 342,000 | **98.9%** |
| 口播 chat | 3,900 | 1.1% |
| 兴趣补算 | ~200（一次性） | ≈0% |
| **合计** | **≈ 346,000** | 100% |

> **核心结论：embedding 是绝对的消耗大头（~99%），chat 口播几乎可忽略。**

## 四、不同使用场景估算

### 场景 A：默认配置，有 LLM Key

| 周期 | tokens | 费用（gpt-4o-mini + text-embedding-3-small） |
|---|---|---|
| 每天 | ~346K | 约 ¥0.03 |
| 每月 | ~10.4M | 约 ¥0.9 |
| 每年 | ~126M | 约 ¥11 |

> 参考价：gpt-4o-mini 输入 ¥0.0005/1K，输出 ¥0.0015/1K；text-embedding-3-small ¥0.00002/1K。

### 场景 B：关闭部分平台（仅知乎+B 站）

采集次数降为 240 次/天，embedding 降至 ~180K/天，**每月约 ¥0.5**。

### 场景 C：增大采集间隔（各平台 ×2）

采集次数减半，embedding 降至 ~171K/天，**每月约 ¥0.45**。

### 场景 D：无 LLM Key（降级模式）

embedding 用占位哈希向量，chat 用模板文案，**token 消耗 = 0**，但语义去重和个性化精度下降。

## 五、优化建议

按成本效益排序：

1. **增大采集间隔**：效果最直接。热榜 30 分钟内变化不大，将所有平台间隔设为 30 min，embedding 消耗降 60%+。
2. **embedding 去重缓存**：当前每条标题每次采集都重算 embedding。可加一层"标题→向量"缓存（24h TTL），重复标题命中缓存不调 API。预计减少 50-70% embedding 调用。
3. **关闭低价值平台**：微博/抖音/小红书若主人不感兴趣，在 config 中 `enabled = false`。
4. **标题截断**：embedding 前将超长标题截断到前 20 字，对相似度影响极小。

## 六、监控方式

运行时日志中可观察：
- `[collector] embedding failed` — embedding 调用失败（降级占位）
- `[agent] computing embeddings for N interests` — 兴趣向量补算
- `[agent] LLM failed` — chat 失败（降级模板）

可用 `deskpet-dump` 导出 `fetch_logs` 表统计每日采集次数，反推 embedding 调用量。
