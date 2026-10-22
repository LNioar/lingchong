# DeskPet

> 一个基于热点与兴趣画像、用宠物口吻分享信息的应用。目前提供 Windows 桌宠界面与 Android 前台全屏界面。

**使用入口：[产品使用说明书](docs/user-manual.md) · [未来扩展计划](docs/roadmap.md)**

文档基线：2026-10-22。功能以当前源码为准，修改后需要重新编译安装；语音仍是部分实现，不能按完整跨平台语音能力使用。

DeskPet 是一个用 Rust 构建的主动式桌面助手。它不等待你提问，而是根据你的兴趣从各大平台抓取热点，筛选出你可能感兴趣的内容，用自然的口吻主动告诉你。

## ✨ 特性

- **多平台热点采集**：知乎、B 站、微博及 RSSHub 相关路由；可用性受网络、限流和采集源影响
- **双击实时刷新**：只从本次网络返回内容中选择候选，失败不复用旧消息；按热点 ID 持久化记录已播报内容
- **向量匹配与去重基础**：支持可选 embedding，未启用时使用占位向量；尚不保证同一事件跨平台只播一次
- **兴趣画像学习**：根据你的反馈（喜欢/不感兴趣/关闭）动态调整兴趣权重，每日自动衰减
- **智能打分**：`score = 0.6×相似度 + 0.2×热度 + 0.15×新鲜度 + 0.05×惩罚`
- **热点回复**：LLM 结合宠物/用户画像自由表达；无 Key 使用模板，模型调用失败或空输出可降级，完整请求超时会提示
- **打扰策略**：支持勿扰与主动播报间隔；启动策略初始化仍有已知差异，见使用说明书
- **TTS 基础实现**：依赖外部 edge-tts 与播放器，部分入口调用；双击尚未接入语音，Android 原生 TTS 尚未实现
- **平台 UI**：Windows 透明桌宠、独立滚动气泡、托盘设置；Android 固定全屏界面与内部滚动气泡
- **宠物画像**：配置名称、性格、表达偏好与背景，下一次模型回复使用；不会自动更换动画资源
- **情绪联动**：idle / happy / curious / excited / sleepy / bored 六种状态
- **配置热重载**：模型、宠物画像等支持更新；语音与 RSSHub 地址仍需重启，间隔在后续循环读取
- **日志轮转**：日志写入文件，超过 10MB 自动轮转，保留 5 份备份

## 🚀 快速开始

### 环境要求

- Rust 工具链（以当前锁定依赖实际要求为准）
- 系统依赖（仅 Tauri GUI 模式需要）：
  - **Linux**：`webkit2gtk-4.1-dev`、`libgtk-3-dev`、`libsoup-3.0-dev`、`libayatana-appindicator3-dev`
  - **Windows / macOS**：需要相应平台的构建依赖；Windows GUI 还需要可用的 WebView2 环境

### 安装系统依赖（Linux）

```bash
# Ubuntu / Debian
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libayatana-appindicator3-dev
```

### 配置 LLM（可选但推荐）

```bash
export DESKPET_LLM_KEY=sk-your-api-key
# 也可在运行后通过设置界面填写；环境变量只参与默认配置构造，不覆盖已有文件里的 Key
```

不配置也能运行——embedding 降级为占位向量，口播降级为模板文案。

### 运行

```bash
# 桌面宠物模式（需要系统依赖）
cargo run -p deskpet-app --features tauri

# 后台 headless 模式（无 GUI，日志输出到终端和文件）
cargo run -p deskpet-app
```

首次运行会自动在数据目录创建 `config.toml` 和 `deskpet.db`：

| 平台 | 数据目录 |
|---|---|
| Linux | `~/.local/share/deskpet/` |
| macOS | `~/Library/Application Support/deskpet/` |
| Windows | `%APPDATA%\deskpet\` |
| Android | 系统分配的应用私有数据目录，由应用管理 |

## 📂 项目结构

```
deskpet/
├── crates/
│   ├── deskpet-core/        # 共享类型：HotItem、UserProfile、Mood、错误、EmbeddingProvider
│   ├── deskpet-storage/     # SQLite 存储：热点/画像/反馈/向量 增删改查
│   ├── deskpet-collector/   # 采集：多平台 Adapter、去重、embedding 入库
│   ├── deskpet-scheduler/   # 调度：定时采集、勿扰策略、每日维护
│   ├── deskpet-agent/       # Agent：打分、口播生成、Function Calling、画像补向量
│   ├── deskpet-tts/         # 语音：Edge TTS
│   ├── deskpet-app/         # 应用入口：配置、IPC、GUI(Tauri)、headless、日志轮转
│   └── deskpet-dump/        # 调试工具：导出数据库热点为 JSON
├── Cargo.toml               # workspace
└── docs/                    # 详细文档
```

## 🔧 常用命令

```bash
# 构建
cargo build --workspace
cargo build -p deskpet-app --features tauri

# 测试（数量随版本变化）
cargo test --workspace

# 导出数据库内容（调试用）
cargo run -p deskpet-dump
```

## 📖 详细文档

- [产品使用说明书](docs/user-manual.md) — 当前功能、平台差异、设置与使用限制
- [未来扩展计划](docs/roadmap.md) — 语音闭环、配置一致性、后台能力与交付验收
- [Windows 指南](docs/windows-guide.md) — Windows 运行、数据与 MSI 构建
- [Android Studio 指南](docs/android-studio.md) — SDK/NDK 与 APK 构建排障
- [架构设计](docs/architecture.md) — 模块职责、数据流、打分与去重原理
- [配置参考](docs/configuration.md) — config.toml 全部字段说明
- [开发者指南](docs/developer-guide.md) — 开发、测试、构建打包、故障排查

## 📄 License

MIT
