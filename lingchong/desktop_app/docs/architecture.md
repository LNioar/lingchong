# 架构设计

## 总体架构

DeskPet 采用 Rust workspace 多 crate 分层架构，各层职责单一、依赖单向：

```
┌─────────────────────────────────────────────────────────┐
│                    deskpet-app (入口层)                   │
│  config │ logging │ runtime │ ipc │ gui(Tauri) │ headless │
└──────────────┬───────────────────┬──────────────────────┘
               │                   │
       ┌───────▼────────┐  ┌───────▼────────┐
       │ deskpet-agent  │  │ deskpet-scheduler │
       │ 打分·口播·画像  │  │ 定时·勿扰·维护    │
       └───────┬────────┘  └───────┬────────┘
               │                   │
       ┌───────▼───────────────────▼────────┐
       │        deskpet-collector           │
       │   多平台采集 · 去重 · embedding入库  │
       └───────────────┬─────────────────────┘
                       │
              ┌────────▼────────┐
              │ deskpet-storage │
              │  SQLite CRUD    │
              └────────┬────────┘
                       │
              ┌────────▼────────┐
              │  deskpet-core   │
              │  共享类型/错误   │
              └─────────────────┘
```

依赖方向：`app → {agent, scheduler, collector, storage, tts} → core`，`collector ↔ agent` 都依赖 `core` 的 `EmbeddingProvider` trait，避免循环依赖。

---

## 核心数据流

一次完整的"主动播报"循环：

```
Scheduler.tick
   ├── collector.fetch_one(platform)
   │     ├── Adapter 抓取原始数据
   │     ├── 批量 embedding（失败降级占位向量）
   │     └── dedup → 精确去重 / 语义近邻去重 → 入库 hot_items + hot_embeddings
   │
   └── AgentOrchestrator.run_cycle()
         ├── 读取 24h 内候选热点（排除 3 天内 dislike）
         ├── ensure_interest_embeddings（补算缺失的兴趣向量并落库）
         ├── score() 打分 = 0.6*sim + 0.2*heat + 0.15*fresh + 0.05*penalty
         ├── 取最高分，低于阈值 0.6 则放弃
         ├── compose_script()：LLM 生成口播（失败 → template_script 降级）
         └── Scheduler.mark_announced() + TTS 播报
```

用户点击气泡反馈后：
```
feedback(hot_id, action)
   ├── FeedbackRepo::insert 记录反馈
   ├── 取热点 tags → profile.adjust_topic(tag, delta)  调整权重
   └── ProfileRepo::save 落库
```

---

## 模块职责

### deskpet-core
共享层，定义跨 crate 的类型与抽象：

| 模块 | 内容 |
|---|---|
| `types.rs` | `HotItem`、`FeedbackAction`（Click/Like/Ignore/Dismiss/Dislike）、`TrustLevel` |
| `profile.rs` | `UserProfile`、`Interest`（含 embedding 持久化字段）、`adjust_topic`、`decay_weights` |
| `mood.rs` | `Mood` 枚举（idle/happy/curious/excited/sleepy/bored） |
| `embedding.rs` | `EmbeddingProvider` trait（collector 与 agent 共用） |
| `error.rs` | `DeskPetError`（Llm/Network/Parse/Storage） |

### deskpet-storage
SQLite 存储层，所有表通过 `schema.rs` 初始化：

| 表 | 用途 |
|---|---|
| `hot_items` | 热点原始数据（id, platform, title, hot_value, tags, first_seen...） |
| `hot_embeddings` | 热点向量（id FK, embedding BLOB），级联删除 |
| `user_profile` | 画像 JSON 序列化存储 |
| `feedback` | 用户反馈（hot_id, action, created_at） |
| `fetch_logs` | 采集日志（platform, status, item_count, error, duration） |

关键方法：`HotRepo::nearest_neighbor`（向量近邻搜索）、`FeedbackRepo::was_recently_disliked/ignored`、`purge_expired`（生命周期清理）。

### deskpet-collector
采集与去重：

- **AdapterManager**：注册各平台 Adapter，持有 HTTP client 和可选 embedder
- **Adapters**：Zhihu / Bilibili / Weibo（含重试）/ Rsshub（多路由聚合）/ RsshubRoute（抖音、小红书单路由）
- **去重**：
  1. 精确去重：同 id 命中 → `AlreadyExists`
  2. 语义去重：24h 窗口内 `nearest_neighbor` 余弦相似度 > 阈值 → `SameEventAs(id)`
  3. 否则 → `New`
- **embedding 入库**：批量调用 embedder，不持 DB 锁；失败降级占位向量

### deskpet-scheduler
定时调度与打扰策略：

- **Scheduler**：为每个平台 spawn 采集任务，每轮读取当前间隔（支持热重载）
- **AnnouncePolicy**：勿扰时段（支持跨天 `23:00-08:00`）、最小播报间隔、静默模式
- **每日维护**：00:00 执行画像权重衰减（×0.99）+ 热点/向量/日志/反馈过期清理

### deskpet-agent
Agent 决策与文案生成：

- **scoring.rs**：打分公式，热点 embedding 从 `hot_embeddings` 读真实向量，兴趣 embedding 从 `Interest.embedding` 读
- **orchestrator.rs**：编排打分 → 阈值过滤 → 口播生成 → 结果返回
- **llm.rs**：`OpenAiCompat` 实现 `LlmProvider` + `EmbeddingProvider`，支持 function calling
- **functions.rs**：`fetch_hot_detail` 工具，从本地 DB 取热点详情（可溯源，不编造）
- **降级**：LLM 失败 → `template_script`；embedding 失败 → 占位向量

### deskpet-app
应用入口：

- **config.rs**：`AppConfig`（general/fetch/llm/tts），从数据目录 `config.toml` 加载
- **logging.rs**：`RotatingFileLogger`，stderr + 文件双输出，10MB 轮转保留 5 份
- **runtime.rs**：`AppState` 组装所有组件，`reload_config` 热重载 + `start_config_watcher` 自动监听
- **ipc.rs**：Tauri 命令（get_hot_list / feedback / trigger_agent / silence_for / reload_config 等）
- **gui.rs**：Tauri 透明窗口 + 系统托盘 + 菜单
- **frontend/index.html**：SVG 宠物 + 6 种 Mood 动画 + 气泡交互

---

## 设计要点

### 1. Embedding 共享 trait
`EmbeddingProvider` 定义在 `deskpet-core`，collector（入库前算热点向量）和 agent（补算兴趣向量）共用同一 `OpenAiCompat` 实例，避免重复实现和依赖循环。

### 2. 降级优先，永不崩溃
所有外部依赖（LLM、embedding、HTTP）的失败都有降级路径：
- LLM 失败 → 模板文案
- embedding 失败 → 哈希占位向量（归一化 32 维）
- 直采失败 → RSSHub 兜底
- 微博瞬时错误 → 指数退避重试 3 次
- HTTP client 构建失败 → 回退默认 client

### 3. DB 锁不跨 await
所有 `self.db.lock().await` 都在同步块内释放，不在持锁期间执行网络请求（embedding 批量计算在锁外）。

### 4. 配置热重载
间隔用 `std::sync::Mutex` 存储（避免 tokio Mutex 在 runtime 内 `blocking_lock` panic），tick 循环每轮读取当前值，修改 config.toml 后下一轮 tick 即生效。勿扰策略通过 `policy: Mutex` 即时更新。
