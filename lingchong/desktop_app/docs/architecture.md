# 架构设计

更新日期：2026-10-24。目录总览见 [项目结构](project-structure.md)，功能状态见 [实现状态](../implementation-status.md)。

## 分层与启动

应用使用 Rust workspace，业务能力由多个 crate 组合，平台界面位于 deskpet-app。deskpet-core 提供跨模块类型和 EmbeddingProvider trait，collector 与 agent 共用抽象而不形成循环依赖。

| crate | 职责 | 关键文件 |
| --- | --- | --- |
| deskpet-core | 热点、反馈、画像、情绪、错误、向量接口 | types.rs、profile.rs、mood.rs、embedding.rs |
| deskpet-storage | SQLite 表结构、仓储、向量编解码与检索 | schema.rs、repo.rs、vector.rs |
| deskpet-collector | 平台 Adapter、采集管理、标签与去重 | adapters/、manager.rs、tagging.rs、dedup.rs |
| deskpet-scheduler | 各平台定时采集、勿扰与间隔策略、每日维护 | scheduler.rs、policy.rs |
| deskpet-agent | 候选评分、模型/模板回复、工具调用 | scoring.rs、orchestrator.rs、llm.rs、functions.rs |
| deskpet-tts | 语音接口与外部命令适配 | lib.rs、edge_tts.rs |
| deskpet-app | 启动、组件组装、配置、IPC 与各平台界面 | src/、frontend/、gen/android/ |
| deskpet-dump | 知乎和 B站各 Top10 热点 JSON 导出 | src/main.rs |

桌面 main.rs 调用共享库 deskpet_app_lib::run()。lib.rs 在 tauri feature 开启时进入 gui::run，否则创建 Tokio runtime 执行 headless::run；移动端通过共享库的 Tauri mobile_entry_point 导出入口启动。

## 平台界面

| 内容 | Windows / 桌面 | Android |
| --- | --- | --- |
| 宠物 | 透明置顶窗口，index.html | 单 Activity / WebView，index.html 加 mobile.js/css |
| 气泡 | bubble.html 独立窗口；预创建、隐藏后复用 | mobile.js 创建页面内 section；右上角显示 |
| 设置 | 托盘懒创建 settings 窗口，settings.html/css/js | 页面内 dialog，模型与宠物画像 |
| 事件 | bubble:show、pet:mood，bubble_ready 回拉 | 复用同类事件，页面内渲染 |
| 原生层 | 桌面窗口、托盘与菜单 | Manifest 横屏；MainActivity 沉浸式系统栏与前台亮屏 |

gui.rs 使用平台条件编译隔离桌面托盘与独立气泡。Android 不创建桌面气泡窗口；“实体桌宠”指将前台软件显示在实体机身的头部屏幕，不是系统桌面 Overlay。

前端源文件统一放在 crates/deskpet-app/frontend/。tauri.conf.json 的 frontendDist 指向该目录；Android 依赖启用 custom-protocol，包内静态资源不依赖电脑开发服务器，但业务 HTTP 请求仍然需要网络。

## 数据与触发流程

### 定时与自动播报

runtime 组装存储、collector、agent、scheduler、TTS 与配置监听器。采集任务请求平台 Adapter，进行标签处理、向量计算和去重并入库；自动播报循环在策略允许时选择候选、打分并生成回复。没有合适结果时不显示热点气泡。

当前评分公式为 score = 0.6 × 相似度 + 0.2 × 热度 + 0.15 × 新鲜度 + 0.05 × 惩罚项。无兴趣条目时冷启动门槛为 0.3；有兴趣且至少一个正权重兴趣有非空向量时使用配置门槛，否则相似度为 0，并将配置门槛乘以 0.4，例如 0.6 折算为 0.24。语义近邻去重不保证跨平台同一事件绝对只出现一次。

### 手动双击刷新

前端触发 trigger_agent，显示刷新状态并阻止重复请求。后端先发送“我看看~”和 excited 情绪，并行刷新已启用平台，只从本次成功返回的候选 ID 中选择未播报热点，排除近期不感兴趣条目。单源约 20 秒，refresh_and_run_agent 阶段约 45 秒超时；失败不从旧缓存补选。成功回复通过 bubble:show 展示，并以 pet:mood 或消息情绪字段驱动动画。

刷新成功但结果为 None 时，ipc.rs 另外调用 idle_reply → compose_idle_reply，按宠物和主人画像生成自然回应，失败时使用固定回应；该阶段在上述 45 秒计时外，LLM 客户端本身设置 60 秒 HTTP 超时，等待 Agent 锁也可能增加耗时。回应不带热点 ID 或链接，不记为热点反馈。Windows 托盘空结果同样调用该路径，自动周期的空结果仍保持安静。

已生成播报的热点 ID 保存到 announcements，重启后继续参与排除。手动触发跳过自动勿扰与评分门槛；Windows 托盘“立即播报”目前仍走本地候选路径，尚未与双击实时刷新统一。

### 气泡与反馈

Windows 的标题、正文与原文链接在内部滚动，底部按钮固定；阅读时暂停自动隐藏。Android 气泡在右上角自己的布局区域内滚动，新消息从顶部显示；关闭后宠物恢复居中。“听点新鲜事”按钮已隐藏，触摸入口为双击宠物。

feedback 命令记录真实热点的反馈，并按 tags 更新用户兴趣权重。Windows 原文点击和自动隐藏也可形成 click / ignore 反馈；Android 提供喜欢、不感兴趣与关闭。没有热点 ID 的状态消息不记录为热点反馈。缺少标签的内容无法有效更新对应主题。

## 持久化

当前 schema.rs 创建以下表：

| 表 | 用途 |
| --- | --- |
| hot_items | 热点标题、来源、标签、排名和时间等 |
| hot_embeddings | 热点向量；关联 hot_items，级联删除 |
| announcements | 按热点 ID 保存已播报时间；关联热点 |
| fetch_logs | 采集状态、耗时、数量和错误 |
| profile | 用户画像键值存储 |
| feedback | 热点反馈动作与时间 |
| pet_state | 宠物状态键值表；建表不代表每个展示状态均已持久化 |

宠物人设属于 config.toml 的 pet 段，不等同于数据库用户画像。paths.rs 提供数据目录：桌面由系统目录定位，Android 在 GUI setup 的早期注入应用私有路径，随后再初始化配置、日志和数据库。

## 配置与外部能力边界

settings.rs 负责 DTO、输入验证和保存。设置保存可立即更新运行时，文件监听器约每 5 秒检查修改。Windows 前端顺序调用 save_config 和 update_profile，配置与画像并非原子提交，页面已有部分成功提示。模型与宠物画像用于后续请求；采集间隔在后续循环读取；语音与 RSSHub 地址需重启。启动策略初始化的已知差异见 [配置参考](configuration.md)。

LLM Provider 与 functions.rs 保留工具调用接口，但当前热点 compose_script 和空结果 compose_idle_reply 均使用 chat(messages, None)，不携带 tools。不能将工具接口存在等同于当前每次回复都会请求工具。

未配置模型 Key 时用真实热点模板回复；向量不可用时可使用占位向量，不能将其当作真实语义理解。网络失败、空结果与整体超时都可能返回状态提示，不能承诺所有失败都会生成成功回复。

TTS 当前通过外部 edge-tts 及播放器实现。自动、托盘和 headless 有调用，双击尚未调用语音，Android 原生语音未完成。前台保持屏幕亮起不等于后台保活，当前无 Android 前台服务、WorkManager 接入或系统悬浮宠物。

进一步的产品能力与交付边界见 [产品说明书](user-manual.md)、[未来扩展计划](roadmap.md)。
