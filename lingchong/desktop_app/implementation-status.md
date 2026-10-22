# DeskPet 实现方案与完成度报告

> 依据：2026-10-19 工作区源码静态审查（8 个 crate、前端 2 个页面、构建产物与随包文档）。
> 说明：本报告只描述**代码中真实存在的行为**；`docs/` 下部分旧文档已与代码不一致，末尾单独列出。

---

## 0. 结论速览

| 维度 | 结论 |
|---|---|
| 核心业务链路 | ✅ 已打通：采集 → 去重 → 入库 → 打分 → 文案 → 气泡 → 反馈 |
| 桌面宠物 UI | 🟡 源码完成「宠物窗口 + 独立气泡窗口」双窗口改造，但 **Windows GUI 安装包尚未产出** |
| 现有二进制 | ⚠️ 均为旧构建：Windows 只有 headless 版，Linux GUI 版早于双窗口改造 |
| 兴趣学习闭环 | 🟡 有完整代码，但**大多数平台没有 tags，UI 也只回传 like/dislike**，学习效果大打折扣 |
| 构建验证 | ❌ 当前环境无 `cargo`，未能执行编译与 48 个单元测试 |
| Android | ❌ 仅有移植计划文档，无任何实现 |

一句话：**功能骨架完整、可运行，但离「可分发、可学习、可稳定使用」还差一轮收尾。**

---

## 1. 项目目标与边界

### 1.1 目标
- 常驻桌面的宠物，不等用户提问，主动播报可能感兴趣的热点。
- 多平台热点采集、跨平台语义去重、按兴趣画像打分。
- 用 LLM 生成口语化文案，失败可降级，进程永不因外部依赖崩溃。
- 克制打扰：勿扰时段、最小播报间隔、静默模式。

### 1.2 非目标（当前未承诺）
- 不做内容全文抓取/摘要生成（只播标题 + 链接 + LLM 口播）。
- 不做账号体系、云同步。
- 不做移动端（仅有规划）。

---

## 2. 实现方案

### 2.1 分层架构

```
deskpet-app（入口：config / logging / runtime / ipc / gui / headless）
   ├── deskpet-agent       打分、口播生成、兴趣向量补算
   ├── deskpet-scheduler   定时采集、勿扰策略、每日维护
   ├── deskpet-collector   多平台 Adapter、去重、embedding 入库
   ├── deskpet-tts         Edge TTS
   └── deskpet-storage     SQLite CRUD / 向量编解码
          └── deskpet-core 共享类型、trait、错误
```

依赖单向，`EmbeddingProvider` 定义在 core，collector 与 agent 共用同一个 `OpenAiCompat` 实例，避免循环依赖。
参见 [Cargo.toml](file:///workspace/deskpet/Cargo.toml)、[runtime.rs](file:///workspace/deskpet/crates/deskpet-app/src/runtime.rs)。

### 2.2 桌面窗口方案：双窗口（本轮核心改动）

早期是「一个大窗口 + 气泡动态撑开」，需要轮询鼠标位置并反复切换 `setIgnoreCursorEvents`，命中判断复杂且易错。
现改为两个**内容贴合窗口**，彻底去掉光标轮询：

| 窗口 | 尺寸 | 内容 | 行为 |
|---|---|---|---|
| `pet` | 190 × 222 | 仅宠物精灵（180 × 212 + 5px 边距） | 常驻、透明、无边框、置顶、不占任务栏 |
| `bubble` | 300 × 180 | 标题 + 口播 + 查看原文 + 喜欢/不感兴趣/关闭 | 启动时创建并隐藏，播报时定位显示 |

实现位置：[gui.rs](file:///workspace/deskpet/crates/deskpet-app/src/gui.rs#L23-L146)。

关键设计：
- **气泡预创建**：启动即建好并 `visible(false)`，避免每次 `show()` 与前端 `listen()` 的竞态。
- **内容回拉**：`LAST_BUBBLE` 静态缓存最近一次播报，气泡加载后调用 `bubble_ready` 主动拉取，双保险。
- **定位策略**：默认放在宠物正上方；若 `y < 0` 则改放下方（[place_bubble](file:///workspace/deskpet/crates/deskpet-app/src/gui.rs#L92-L106)）。
- **命中策略**：按约定使用**矩形命中**，不做宠物像素级轮廓判断；两个窗口内容几乎填满，透明空白很少。
- **权限收敛**：[default.json](file:///workspace/deskpet/crates/deskpet-app/capabilities/default.json) 只保留 `start-dragging`，已删除 `set-ignore-cursor-events`、`cursor-position` 等。

播报事件流：

```
定时循环 / 托盘「立即播报」/ 双击
        ↓
run_agent_cycle[_force]()
        ↓ Some(AnnounceResult)
announce_to_ui()
   ├── 写入 LAST_BUBBLE
   ├── 定位并 show 气泡窗口
   ├── emit "bubble:show"
   └── emit "pet:mood" → 宠物切情绪
```

### 2.3 端到端数据流

```
Scheduler 定时 tick
  └─ collector.fetch_one(platform)
       ├─ Adapter 抓取
       ├─ 批量 embedding（失败→32 维哈希占位向量）
       ├─ 精确去重（同 id）
       ├─ 语义去重（24h 内余弦 ≥ 0.85）
       └─ upsert hot_items + hot_embeddings + fetch_logs

Agent 周期
  ├─ 取窗口内候选（hard-exclude 3 天内 dislike）
  ├─ 补算缺失的兴趣 embedding 并落库
  ├─ score = 0.6×sim + 0.2×heat + 0.15×fresh + 0.05×penalty
  ├─ 无画像用冷启动阈值 0.3，有画像用配置阈值（默认 0.6）
  ├─ LLM 生成 1-2 句口播（失败/空/过渡语 → 重试一次 → 模板文案）
  └─ mark_announced + TTS + 气泡

用户反馈
  ├─ feedback(hot_id, like/dislike)
  ├─ feedback 表落库
  ├─ 按热点 tags 调整画像权重（like +0.12 / dislike -0.10）
  ├─ 画像落库 + 宠物情绪联动
  └─ 隐藏气泡

每日 00:00
  ├─ 画像权重 ×0.99
  ├─ 热点/向量 7 天清理
  ├─ 采集日志/反馈 30 天清理
  └─ 孤儿向量清理
```

### 2.4 降级与可靠性

| 外部依赖 | 失败时行为 |
|---|---|
| LLM chat | 重试一次 → 模板文案（含标题、平台、排名、链接） |
| Embedding | 32 维标题哈希占位向量，归一化 |
| 直采平台 | RSSHub 对应路由兜底 |
| 微博瞬时错误 | 指数退避重试 3 次（1s/2s/4s） |
| HTTP client 构建失败 | 回退默认 client |
| TTS | 记录警告，不影响播报文案 |
| 配置解析失败 | 内存使用默认值，**不覆盖用户文件** |

---

## 3. 已实现清单

### 3.1 数据采集

| 能力 | 状态 | 位置 |
|---|---|---|
| 知乎热榜直采 | ✅ | [zhihu.rs](file:///workspace/deskpet/crates/deskpet-collector/src/adapters/zhihu.rs) |
| B 站热门直采（含分区 tag） | ✅ | [bilibili.rs](file:///workspace/deskpet/crates/deskpet-collector/src/adapters/bilibili.rs) |
| 微博热搜直采 + 3 次退避重试 | ✅ | [weibo.rs](file:///workspace/deskpet/crates/deskpet-collector/src/adapters/weibo.rs) |
| RSSHub 多路由聚合 | ✅ | [rsshub.rs](file:///workspace/deskpet/crates/deskpet-collector/src/adapters/rsshub.rs) |
| 抖音 / 小红书（RSSHub 单路由，独立周期） | ✅ | 同上 |
| 直采失败自动回退 RSSHub | ✅ | [manager.rs](file:///workspace/deskpet/crates/deskpet-collector/src/manager.rs#L139-L170) |
| 平台级限频 | ✅ | 最小间隔检查，见 manager |
| 采集日志落库 | ✅ | `fetch_logs` |

### 3.2 去重与存储

| 能力 | 状态 | 说明 |
|---|---|---|
| 确定性 id：sha256(platform + title) | ✅ | [types.rs](file:///workspace/deskpet/crates/deskpet-core/src/types.rs#L45-L55) |
| 精确去重 | ✅ | 同 id 刷新 `last_seen` |
| 语义去重（24h 窗口 + 余弦阈值 0.85） | 🟡 | 能识别同事件，但**不合并、不丢弃**，见 4.2 |
| 向量 BLOB 编解码 + 余弦 | ✅ | [vector.rs](file:///workspace/deskpet/crates/deskpet-storage/src/vector.rs) |
| 兴趣画像持久化（含 embedding） | ✅ | `profile` 表 JSON |
| 反馈记录 | ✅ | `feedback` 表 |
| 生命周期清理 | ✅ | 热点/向量 7 天，日志/反馈 30 天 |
| 宠物状态表 | 🟡 | `pet_state` 表与 `PetStateRepo` 已建，但**无人调用** |

### 3.3 调度与打扰策略

| 能力 | 状态 |
|---|---|
| 每平台独立采集循环 | ✅ |
| 采集间隔热更新 | ✅（下一轮 tick 生效） |
| 勿扰时段（支持跨天 `23:00-08:00`） | ✅ |
| 最小播报间隔 | ✅ |
| 静默 N 分钟 | ✅（后端命令存在，但无 UI 入口） |
| 每日 00:00 维护任务 | ✅ |
| 配置变更监听（每 5s 比对 mtime） | ✅ |

核心代码：[scheduler.rs](file:///workspace/deskpet/crates/deskpet-scheduler/src/scheduler.rs)、[policy.rs](file:///workspace/deskpet/crates/deskpet-scheduler/src/policy.rs)。

### 3.4 Agent 与 LLM

| 能力 | 状态 | 说明 |
|---|---|---|
| 打分公式 | ✅ | `0.6 sim + 0.2 heat + 0.15 fresh + 0.05 penalty` |
| 兴趣向量缺失时补算并落库 | ✅ | `ensure_interest_embeddings` |
| 冷启动低阈值 | ✅ | 无画像时 0.3 |
| 3 天内 dislike 硬排除 | ✅ | 候选过滤 |
| 近期 ignore/dismiss/dislike 软惩罚 | ✅ | 0.3 系数 |
| LLM 口播 + 过渡语过滤 + 重试 + 模板降级 | ✅ | [orchestrator.rs](file:///workspace/deskpet/crates/deskpet-agent/src/orchestrator.rs) |
| Persona 合成（无画像时按兴趣权重生成） | ✅ | |
| OpenAI 兼容 chat / embeddings | ✅ | [llm.rs](file:///workspace/deskpet/crates/deskpet-agent/src/llm.rs) |
| Function Calling 工具定义与本地执行 | 🟡 | 基础设施与测试已写，但**主链路明确不使用 tools**（避免 DeepSeek 空返回） |
| 数据可信度加权 | ❌ | `TrustLevel` 已落库，但打分未使用 |

### 3.5 桌面 UI 与交互

| 能力 | 状态 | 说明 |
|---|---|---|
| 透明无边框置顶宠物窗口 | ✅ | 190×222 |
| 独立气泡窗口 | ✅ | 300×180，预创建、定位、显示/隐藏 |
| 拖拽移动 | ✅ | 位移 > 3px 触发 `startDragging` |
| 双击立即播报 | ✅ | 手动检测 400ms 内两次点击 |
| 滚轮切换 6 种情绪 | ✅ | idle/happy/curious/excited/sleepy/bored |
| 情绪事件联动 | ✅ | `pet:mood` |
| 气泡「查看原文」走系统浏览器 | ✅ | `open_url`，仅允许 http/https |
| 喜欢 / 不感兴趣 / 关闭 | ✅ | 前两者写反馈并联动情绪 |
| 气泡 15s 自动隐藏 | ✅ | |
| 气泡加载后回拉最近播报 | ✅ | `bubble_ready` + `LAST_BUBBLE` |
| 系统托盘菜单（显示/隐藏/立即播报/退出） | ✅ | 左键单击切换显示 |
| 精灵图动画引擎（多情绪切片、帧率、缩放） | 🟡 | 引擎完整，但仓库只有 `idle.png`，其余情绪会继续播放上一张精灵图；无图时才用 SVG 动画 |
| SVG 宠物 6 种情绪动画 | ✅ | 作为无精灵图/无对应切片时的兜底 |

前端：[index.html](file:///workspace/deskpet/crates/deskpet-app/frontend/index.html)、[bubble.html](file:///workspace/deskpet/crates/deskpet-app/frontend/bubble.html)。

### 3.6 工程能力

| 能力 | 状态 |
|---|---|
| 配置文件自动生成 / 读写 | ✅ `%APPDATA%\deskpet\config.toml` |
| 配置热重载（策略、间隔、LLM/Embedding、阈值） | ✅ |
| 日志文件轮转（10MB × 5 份）+ stderr | ✅ |
| headless 后台模式 | ✅ |
| Windows release 隐藏控制台 | ✅（[main.rs](file:///workspace/deskpet/crates/deskpet-app/src/main.rs#L1-L3)） |
| 调试导出工具 | 🟡 仅导出知乎/B站各 Top10；README 所称“反馈/采集日志”未实现 |
| 单元测试 | ✅ 静态统计 48 个（core 7、collector 8、agent 14、scheduler 11、storage 8）；本次环境无 cargo，未执行 |
| GUI / 集成 / E2E 测试 | ❌ |

---

## 4. 未实现与部分实现

### 4.1 功能缺口

| 优先级 | 功能 | 现状 | 缺口 |
|---|---|---|---|
| **P0** | Windows 安装包 | 无 MSI/NSIS 产物 | 需在 Windows + MSVC 下 `tauri build`；当前分发只有 headless exe |
| **P0** | 反馈学习有效性 | 仅 B 站条目带 `tags`，其余平台 tags 为空 | 点赞/点踩多数情况下调整 0 个兴趣，画像几乎学不到东西 |
| **P0** | 勿扰/无内容时的气泡行为 | 定时循环无条件 `announce_to_ui` | 勿扰时段或无候选时仍会每 45 分钟弹「暂时没有值得说的热点」 |
| **P1** | 反馈类型完整性 | UI 只发 like/dislike | `click`、`ignore` 从不产生；15s 自动消失也不记 ignore |
| **P1** | Windows 语音播报 | `edge-tts` CLI + mpv/ffplay/cvlc | Windows 默认三者都没有 → 实际无声音；也未直连微软 TTS |
| **P1** | 语义去重落地 | `SameEventAs` 被忽略 | 同事件跨平台仍分别入库，可能重复播报（force 模式更明显） |
| **P1** | 单实例 | 无 single-instance 插件 | 重复启动会出现多只宠物、并发写同一 SQLite |
| **P2** | 热点列表 / 画像 / 设置界面 | IPC 命令存在（`get_hot_list` / `get_profile` / `update_profile` / `silence_for` / `tts_speak` / `reload_config`） | 前端从未调用，全部是“有后端无入口” |
| **P2** | 宠物位置持久化 | `pet_state` 表已建 | 重启后位置丢失（表与 Repo 未被使用） |
| **P2** | 开机自启 / 自动更新 | 无 | 需系统集成 |
| **P2** | 其余 5 种情绪精灵图 | 仅 `idle.png` | 需美术资源；引擎无需改动 |
| **P2** | embedding 缓存 | 每次采集全量重算 | token 消耗大头（估算 ~99%），可按「标题→向量」24h 缓存 |
| **P2** | 标签抽取 | 无 | 可从标题/平台字段抽取或让 LLM 打标，是修复学习闭环的关键前置 |
| **P3** | 可信度加权 | `TrustLevel` 未参与打分 | 直采/聚合源权重相同 |
| **P3** | Android 移植 | 仅 `android-porting-plan.md` | 数据目录、TTS、托盘、窗口、触摸交互全需改造 |

### 4.2 已知缺陷 / 代码问题

| 编号 | 问题 | 影响 | 位置 |
|---|---|---|---|
| B1 | 定时循环即使 `ann` 为 `None` 也调用 `announce_to_ui` | 勿扰时段/无内容仍弹气泡，破坏打扰策略 | [gui.rs](file:///workspace/deskpet/crates/deskpet-app/src/gui.rs#L244-L252) |
| B2 | 双击触发不调用 `speak` | 双击有气泡无语音；托盘「立即播报」却有语音，行为不一致 | [ipc.rs](file:///workspace/deskpet/crates/deskpet-app/src/ipc.rs#L96-L105) |
| B3 | `AppState.config` 重载后不更新 | GUI 主循环仍用启动时的 `min_announce_interval_min` 睡眠；改配置后实际节奏不变 | [runtime.rs](file:///workspace/deskpet/crates/deskpet-app/src/runtime.rs#L392-L403) |
| B4 | 配置解析失败 → 内存全量默认值 | 用户改错一个字段，`api_key` 等在内存中失效，LLM/embedding 静默降级 | [config.rs](file:///workspace/deskpet/crates/deskpet-app/src/config.rs#L143-L155) |
| B5 | `get_profile` / `update_profile` 在 async 命令里直接 `blocking_lock` | 一旦前端调用可能 panic（`list_hot` 已用 `spawn_blocking` 规避） | [ipc.rs](file:///workspace/deskpet/crates/deskpet-app/src/ipc.rs#L54-L66)、[runtime.rs](file:///workspace/deskpet/crates/deskpet-app/src/runtime.rs#L232-L241) |
| B6 | `open_url` 用 `cmd /C start` 且只做前缀校验 | URL 中的 `&` 等字符可能被 cmd 解释，存在命令注入风险 | [ipc.rs](file:///workspace/deskpet/crates/deskpet-app/src/ipc.rs#L146-L165) |
| B7 | 气泡定位只判断 `y < 0` | 多显示器负坐标会误判；无水平/底部边界 clamp | [gui.rs](file:///workspace/deskpet/crates/deskpet-app/src/gui.rs#L92-L106) |
| B8 | `default_interval_min`、`tts.provider` 从未被读取 | 配置项看似可调，实际无效 | [config.rs](file:///workspace/deskpet/crates/deskpet-app/src/config.rs#L23-L54) |
| B9 | `AlreadyExists` 分支设置 `first_seen = 0` 后 upsert | upsert 冲突时只更新 `last_seen/rank`，该赋值无效 | [manager.rs](file:///workspace/deskpet/crates/deskpet-collector/src/manager.rs#L189-L193) |
| B10 | LLM 请求体/响应体以 info 级别完整写日志 | 日志体积大，且包含用户画像与播报内容 | [llm.rs](file:///workspace/deskpet/crates/deskpet-agent/src/llm.rs#L74-L89) |
| B11 | release 隐藏控制台后，panic 信息不可见 | 只落到日志文件，用户难以反馈崩溃 | [main.rs](file:///workspace/deskpet/crates/deskpet-app/src/main.rs#L3) |
| B12 | `rust-version = "1.75"` 与实际依赖不匹配 | Tauri 2.12 的 MSRV 更高，低版本工具链会直接构建失败 | [Cargo.toml](file:///workspace/deskpet/Cargo.toml#L17) |

### 4.3 构建与发布状态

| 产物 | 状态 | 证据 |
|---|---|---|
| 源码（双窗口版） | ✅ 完整 | `gui.rs` / `bubble.html` 更新于 2026-10-16，`main.rs` 更新于 2026-10-18 |
| Windows x64 可执行文件 | 🟡 仅 headless，且早于双窗口改造 | `dist/deskpet-latest/bin/deskpet-app.exe`（15.9MB，2026-10-15） |
| Windows GUI 二进制 | ❌ 不存在 | 交叉构建目录未包含 tauri/webview2 依赖 |
| MSI / NSIS 安装包 | ❌ 不存在 | 工作区无 `.msi` / `setup.exe` |
| Linux GUI 二进制 | 🟡 旧版（2026-10-07，早于双窗口改造） | `dist/deskpet-linux-x64/deskpet-app-gui` |
| Linux deb / AppImage | ❌ 不存在 | — |
| macOS 产物 | ❌ 未验证 | — |
| 源码分发包 | ✅ | `dist/deskpet-latest.zip`、`dist/deskpet-windows-x64-src.zip`、`/workspace/deskpet-src.zip` |
| 用户指南进分发包 | ❌ | `用户使用指南.md` 在项目根目录，未打进 `dist/*` |

> 当前环境执行 `cargo` 返回 `command not found`，因此**本报告不声称编译通过或测试通过**。

### 4.4 文档漂移（需随代码更新）

| 文档 | 陈旧内容 | 代码实际 |
|---|---|---|
| [configuration.md](file:///workspace/deskpet/docs/configuration.md#L11-L22) | 称 `llm.*` 修改需重启 | 已支持 LLM/embedding 热更新 |
| [architecture.md](file:///workspace/deskpet/docs/architecture.md#L121-L130) | 称气泡在 `index.html` 内、单窗口 | 已拆为 `bubble.html` 独立窗口 |
| [README.md](file:///workspace/deskpet/README.md#L9-L10) | 称语义去重可保证“同一事件只播一次” | 仅标记 `SameEventAs`，未合并 |
| [README.md](file:///workspace/deskpet/README.md#L91-L93) | 称 dump 导出热点/反馈/日志 | 实际只导出知乎/B站各 Top10 |
| [windows-guide.md](file:///workspace/deskpet/docs/windows-guide.md#L12) | 称发布包仅 headless | 与现状一致，但容易与「安装包」预期混淆 |

---

## 5. 风险与限制

1. **安全**
   - API Key 明文存于 `config.toml`，无系统密钥链保护。
   - `csp: null`，WebView 无内容安全策略限制（当前只加载本地资源，风险可控）。
   - `open_url` 的 cmd 拼接存在注入面，建议改为 Windows API（`ShellExecuteW`）或严格 URL 编码校验。
2. **性能**
   - 向量检索为全表暴力余弦（24h 窗口），数据量增大后线性变慢，无 ANN 索引。
   - 每次采集都重算全部标题 embedding，是 token 成本主要来源。
   - SQLite 单连接 + `tokio::Mutex`，并发采集时存在串行等待。
3. **兼容性**
   - Windows 需 WebView2（Win10/11 通常自带）。
   - MSVC 链接器缺失会直接导致构建失败（用户已实际遇到 `link.exe not found`）。
   - TTS 依赖外部 Python CLI 与播放器，跨平台一致性差。
4. **交互**
   - 矩形命中意味着窗口透明角落、宠物轮廓外区域仍会拦截鼠标点击；这是本轮明确接受的取舍。
   - 气泡固定 300×180、文案 4 行截断，长文案会丢内容。

---

## 6. 建议路线图

### P0：先让「可安装、可播报、可学习」
1. 在 Windows + VS C++ Build Tools 下完成 `cargo tauri build --features tauri`，产出 MSI/NSIS，实机验证双窗口定位、拖拽、托盘、双击。
2. 修复 B1：仅在 `Some(ann)` 时调用 `announce_to_ui`；`None` 时保持安静。
3. 打通标签来源：为知乎/微博/RSSHub/抖音/小红书补充 `tags`（平台字段、关键词规则或 LLM 打标），让反馈真正能调整画像。
4. 补全反馈：点击原文记 `click`，自动超时记 `ignore`。

### P1：体验与可靠性
5. 双击播报接入 TTS；Windows 改用系统播放或 HTTP 直连 TTS，不再依赖 `edge-tts` CLI + mpv。
6. 语义去重落地：`SameEventAs` 时合并到已有 id 或直接丢弃，避免重复入库/重复播报。
7. 单实例守卫，避免多宠物并发写库。
8. 修复 B3/B4/B5：配置快照可更新、解析失败保留旧配置、移除 async 中的 `blocking_lock`。

### P2：产品化
9. 设置界面：API Key、平台开关、勿扰时段、语音开关（消灭手改 config.toml）。
10. 热点列表/画像面板，复用已有 IPC。
11. 宠物位置持久化、开机自启、系统通知。
12. embedding 标题缓存 + 增量计算，降低成本。
13. 补齐 5 种情绪精灵图；Android 按计划推进。

---

## 7. 验证状态说明

| 项目 | 状态 |
|---|---|
| 源码完整性 | ✅ 8 个 crate + 2 个前端页面 + 能力配置齐全 |
| 静态一致性审查 | ✅ 本报告逐文件核对 |
| `cargo build` | ❌ 当前沙箱无 cargo，未执行 |
| `cargo test --workspace` | ❌ 未执行；仅静态统计 48 个 `#[test]` / `#[tokio::test]` |
| Windows 实机运行 | ❌ 未验证（缺 MSVC、缺 GUI 产物） |
| 双窗口定位/焦点 | ❌ 未验证（需 Windows 实机拖到屏幕四角测试） |
| 安装包 | ❌ 未产出 |

---

*附：本报告为现状快照，建议每次发布前更新「4.3 构建与发布状态」与「4.2 已知缺陷」两节。*
