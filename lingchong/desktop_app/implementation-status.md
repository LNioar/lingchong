# DeskPet 当前实现状态

更新日期：2026-10-24。依据当前 `desktop_app/` 源码、配置与前端回归脚本整理。本文替代 2026-10-19 的旧状态报告；不将早期编译记录视为当前版本完整交付证明。

## 项目组成

Rust workspace 共 8 个 crate。应用包含共享库入口、桌面可执行入口、headless 模式、静态前端及已有 Android 工程。详见 [项目结构](docs/project-structure.md)、[架构设计](docs/architecture.md)。

## 已实现能力

| 范围 | 当前源码行为 | 主要入口 |
| --- | --- | --- |
| 采集 | 知乎、B站、微博及 RSSHub 相关路由，标签提取、去重、向量入库 | `deskpet-collector/src/` |
| 存储 | 热点、向量、反馈、画像、采集日志、已播报记录与宠物状态表 | `deskpet-storage/src/` |
| 回复 | 打分、模型/模板热点回复、无热点自然回应、宠物画像；保留详情工具接口，当前回复路径不传 tools | `deskpet-agent/src/` |
| 手动刷新 | 双击联网刷新，仅从本次成功返回的未播报候选选择；失败不回放旧消息 | `deskpet-app/src/ipc.rs`、`runtime.rs` |
| Windows | 透明宠物、独立滚动气泡、托盘、可复用设置窗口 | `gui.rs`、`frontend/bubble.html`、`settings.*` |
| Android | 横屏、沉浸全屏、前台亮屏、深色界面、待机宠物居中、消息右上角气泡 | `frontend/mobile.*`、Android Manifest 与 MainActivity |
| Android 交互 | 双击触摸、键盘触发、请求防重入、内部滚动、模型与宠物设置；单击热点按钮已隐藏 | `frontend/mobile.js` |
| 动画 | 六种情绪精灵、SVG 后备、临时情绪恢复 | `frontend/index.html`、`sprites/` |
| 配置 | TOML、设置验证、模型与宠物画像热更新、密钥保留/替换/清除 | `config.rs`、`settings.rs`、`runtime.rs` |
| 日志与诊断 | 文件轮转、诊断脚本、有限热点 JSON 导出 | `logging.rs`、`scripts/`、`deskpet-dump` |
| 实体桌宠 | 已有外观概念图与 Android 头部屏幕设计，尚不是量产硬件 | [设计说明](docs/design/desktop-companion.md) |

源码入口中的 `runtime.rs`、`gui.rs` 等均位于 `crates/deskpet-app/src/`。

## 部分实现与待完成

- 语音仍依赖外部 `edge-tts` 与播放器；双击未调用语音，Android 原生 TTS 未接入。
- Android 为前台应用，未完成前台服务、后台常驻、系统悬浮宠物与开机启动。
- Android 设置只覆盖模型与宠物画像，尚不等同于 Windows 完整设置。
- Windows 托盘“立即播报”与双击实时刷新仍有行为差异；启动时自定义勿扰策略初始化存在已知限制。
- 双击空结果会生成宠物口吻回应，该阶段在 45 秒刷新超时之外；不能承诺整个操作 45 秒内结束。自动空结果仍保持安静。
- Windows 设置配置与用户画像分步保存，页面可提示部分成功，尚无跨配置文件和数据库的原子提交。
- 语义去重不保证同一事件跨平台只出现一次；接口可用性、网络、平台标签和模型能力影响内容质量。
- 亮屏仅在 Activity 前台生效，不代表 CPU 唤醒锁、后台保活或设备管理模式。
- 根目录 `lingchong.msi`、`target/`、Android `build/` 中的产物不能仅凭存在认定为本次源码构建。

## 验证与交付边界

前端回归入口为 `node scripts/test-pet-ui.cjs` 与 `node scripts/test-bubble-ui.cjs`；Rust 测试入口为 `cargo test --workspace`。测试数量会变化，本文不再使用旧版固定“48 项”统计。

此前界面修改已完成前端逻辑回归，但横屏设备效果、前台亮屏、设置与安装升级仍需新 APK 真机验收。本轮文档维护不重新生成 MSI / APK，不声明完成全量 Rust、Gradle 或硬件验证。

后续工作见 [未来扩展计划](docs/roadmap.md)，日常使用见 [产品使用说明书](docs/user-manual.md)。
