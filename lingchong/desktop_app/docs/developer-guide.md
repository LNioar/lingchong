# 开发者指南

## 环境准备

```bash
# Rust
rustup toolchain install stable   # 需要 1.75+

# Linux 系统依赖（Tauri GUI 模式）
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libayatana-appindicator3-dev
```

## 项目布局

```
crates/
├── deskpet-core/        # 共享类型与 trait
├── deskpet-storage/     # SQLite 存储
├── deskpet-collector/   # 采集与去重
├── deskpet-scheduler/   # 调度与打扰策略
├── deskpet-agent/       # 打分与口播
├── deskpet-tts/         # TTS
├── deskpet-app/         # 入口（config/logging/runtime/ipc/gui/headless）
└── deskpet-dump/        # 调试工具
```

## 构建

```bash
# 全部 crate
cargo build --workspace

# 仅 headless
cargo build -p deskpet-app

# Tauri GUI
cargo build -p deskpet-app --features tauri

# 发布构建
cargo build --workspace --release
```

## 运行

```bash
# headless（后台服务模式）
cargo run -p deskpet-app

# Tauri GUI
cargo run -p deskpet-app --features tauri

# 无 GUI 时验证核心逻辑（推荐开发调试）
cargo run -p deskpet-app 2>&1 | grep -E "fetched|ANNOUNCE|tick"
```

## 测试

```bash
# 全部测试
cargo test --workspace

# 单 crate
cargo test -p deskpet-agent
cargo test -p deskpet-scheduler

# 详细输出
cargo test -p deskpet-collector -- --nocapture
```

当前测试覆盖（48 个）：

| 模块 | 测试数 | 覆盖点 |
|---|---|---|
| core | 7 | 画像增删/权重边界/衰减/兴趣筛选/反馈 delta |
| collector | 8 | 去重 New/Exists/跨平台同事件/无emb/24h窗口/精确优先 |
| agent | 14 | 打分公式（相似度/热度/新鲜度/惩罚）+ LLM降级/mock/模板/工具溯源 |
| scheduler | 11 | 勿扰时段/最小间隔/静默/综合判断 |
| storage | 8 | dislike排除/过期清理/孤儿向量/反馈过期/日志过期 |

## 打包

```bash
# Tauri 打包（生成安装包）
cargo tauri build

# 仅构建二进制
cargo build -p deskpet-app --features tauri --release
```

产物位于 `target/release/bundle/`。

## 调试

### 查看数据库内容

```bash
cargo run -p deskpet-dump
```

导出 `hot_items`、`feedback`、`fetch_logs` 为 JSON。

### 日志

日志同时输出到 stderr 和文件：`~/.local/share/deskpet/deskpet.log`

设置日志级别：
```bash
RUST_LOG=debug cargo run -p deskpet-app
```

### 常用日志关键词

| 关键词 | 含义 |
|---|---|
| `[scheduler] fetch tick` | 定时采集触发 |
| `[scheduler] agent tick` | Agent 周期触发 |
| `[scheduler] daily tick` | 每日维护触发 |
| `[zhihu] fetched N new items` | 采集结果 |
| `[agent] top candidate score=` | 打分结果 |
| `[agent] below threshold, skip` | 低于阈值跳过 |
| `[agent] LLM failed, fallback to template` | LLM 降级 |
| `[watcher] config changed, reloading` | 配置热重载 |
| `[feedback] ... adjusted N topics` | 反馈回写画像 |
| `[daily] maintenance complete` | 每日维护完成 |

## 扩展开发

### 新增采集平台

1. 在 `deskpet-collector/src/adapters/` 新增 `XxxAdapter`，实现 `HotAdapter` trait
2. 在 `manager.rs` 的 `AdapterManager` 中注册
3. 在 `config.rs` 默认 `platforms` 中添加配置
4. 在 `runtime.rs` 中 `collector.register(Arc::new(XxxAdapter))`

### 新增 TTS 提供方

1. 在 `deskpet-tts/src/` 新增实现 `TtsProvider` trait
2. 在 `runtime.rs` 中根据 `config.tts.provider` 选择

### 新增情绪状态

1. 在 `deskpet-core/src/mood.rs` 添加枚举值
2. 在 `frontend/index.html` 中添加对应 CSS 动画
3. 在 `orchestrator.rs` 中根据分数映射情绪

## 故障排查

### 编译错误：`Cannot block the current thread from within a runtime`

不要在 async runtime 内调用 `tokio::sync::Mutex::blocking_lock()`。间隔类共享状态使用 `std::sync::Mutex`。

### Tauri 构建：`webkit2gtk` 相关错误

安装系统依赖：
```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libayatana-appindicator3-dev
```

### 采集全部失败

- 检查网络连接（沙箱环境可能限制外网）
- 微博等平台可能有反爬，会触发重试 + RSSHub 兜底
- 查看 `fetch_logs` 表获取详细错误

### 不播报

- 检查 `quiet_hours` 是否覆盖当前时间
- 检查画像是否有兴趣（无兴趣时 sim=0，难达阈值）
- 查看 `[agent] below threshold, skip` 日志，可临时调低阈值验证

### embedding 不生效

确认 `config.toml` 中 `llm.api_key` 已配置（或 `DESKPET_LLM_KEY` 环境变量）。未配置时日志会显示 `embedding disabled (placeholder fallback)`。
