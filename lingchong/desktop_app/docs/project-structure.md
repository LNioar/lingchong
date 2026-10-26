# 当前项目结构

更新日期：2026-10-24。以下路径均相对于项目根目录 `desktop_app/`。父目录下其他项目副本为备份，不参与开发、资源定位或打包。

```text
desktop_app/
├── Cargo.toml / Cargo.lock       # Rust workspace 与依赖锁定
├── README.md                     # 项目入口
├── 用户使用指南.md                # 产品说明书的快捷入口
├── implementation-status.md      # 当前实现与验证边界
├── crates/
│   ├── deskpet-core/src/          # 类型、画像、情绪、错误、EmbeddingProvider
│   ├── deskpet-storage/src/       # SQLite schema、仓储、向量处理
│   ├── deskpet-collector/src/     # 平台适配器、采集管理、去重、标签提取
│   ├── deskpet-scheduler/src/     # 定时采集、勿扰策略、每日维护
│   ├── deskpet-agent/src/         # 打分、回复编排、模型客户端、详情工具
│   ├── deskpet-tts/src/           # 语音接口与 Edge TTS 外部命令实现
│   ├── deskpet-dump/src/main.rs   # 知乎 / B站各 Top10 热点 JSON 导出
│   └── deskpet-app/
│       ├── Cargo.toml / build.rs
│       ├── tauri.conf.json       # Tauri 配置，frontendDist 指向 frontend
│       ├── capabilities/         # WebView 权限
│       ├── icons/                # 打包图标
│       ├── src/
│       │   ├── main.rs           # 桌面可执行入口，调用共享库
│       │   ├── lib.rs            # 共享启动入口；移动端导出入口
│       │   ├── gui.rs            # Tauri 窗口、桌面托盘、移动端分支
│       │   ├── headless.rs       # 未启用 tauri feature 的运行模式
│       │   ├── ipc.rs            # 前端命令、热点触发、反馈
│       │   ├── settings.rs       # 设置 DTO、验证与保存
│       │   ├── config.rs         # TOML 配置与默认值
│       │   ├── runtime.rs        # 组件组装、运行状态、配置重载
│       │   ├── paths.rs          # 数据目录定位与移动端注入
│       │   └── logging.rs        # 文件日志与轮转
│       ├── frontend/
│       │   ├── index.html        # 宠物页面与精灵动画引擎
│       │   ├── mobile.js/css     # 移动端交互与 Android 实体桌宠界面
│       │   ├── bubble.html       # 桌面独立气泡页面
│       │   ├── settings.html/css/js # 桌面设置页面
│       │   └── sprites/          # 六种情绪 PNG 与资源说明
│       └── gen/android/          # 已有 Android 工程，含项目定制
│           ├── gradlew / gradlew.bat / gradle/
│           ├── build.gradle.kts / settings.gradle
│           ├── buildSrc/         # Rust 构建接入
│           └── app/
│               ├── build.gradle.kts
│               └── src/main/
│                   ├── AndroidManifest.xml # 横屏与 Activity 声明
│                   ├── java/com/deskpet/app/MainActivity.kt
│                   ├── res/      # Android 资源
│                   └── assets/   # 打包资产；前端源文件在 frontend
├── scripts/                      # 维护、诊断与前端回归脚本
├── demos/edge-tts/                # 独立语音实验室：Python 服务、网页与启动脚本
├── docs/                         # 文档索引、使用、架构与计划
│   └── design/                   # 实体桌宠概念图、提示词与设计说明
├── .tools/                       # 本地辅助工具与记录，Git 忽略
├── diagnostics/                  # 诊断输出快照，Git 忽略
├── target/                       # Rust / Tauri 构建产物，Git 忽略
├── lingchong.msi                 # 本地安装包，不代表最新源码版本
└── 桌面智能宠物产品概念展示.png     # 根目录概念参考图
```

## 修改入口与平台边界

| 要修改的内容 | 首选位置 |
| --- | --- |
| 宠物动画、帧数、显示尺寸 | `frontend/index.html` 的 `SPRITE_CONFIG` 及 `frontend/sprites/` |
| Android 屏幕排布、气泡、设置弹窗 | `frontend/mobile.css`、`frontend/mobile.js` |
| Android 横屏 | `gen/android/app/src/main/AndroidManifest.xml` |
| Android 隐藏系统栏、前台亮屏 | `gen/android/app/src/main/java/com/deskpet/app/MainActivity.kt` |
| 桌面独立气泡与阅读交互 | `frontend/bubble.html`、`src/gui.rs` |
| 桌面设置 | `frontend/settings.*`、`src/settings.rs`、`src/ipc.rs` |
| 模型回复与评分 | `deskpet-agent/src/` |
| 数据表、已播报记录 | `deskpet-storage/src/schema.rs`、`repo.rs` |

上表中 `frontend/`、`src/`、`gen/` 均位于 `crates/deskpet-app/` 内。

Android 与桌面共用宠物资源和 Rust 业务。桌面使用独立气泡窗口与托盘设置窗口；Android 使用单 Activity / WebView、页面内右上角气泡和设置弹窗。当前不提供 Android 系统桌面悬浮宠物。

## 源文件、构建产物与运行数据

- `frontend/` 是前端源文件，不要只修改 APK 内资产或 `build/intermediates/`。
- `gen/android/` 虽由 Tauri 初始化，现已包含横屏、沉浸模式、Gradle 和 Rust 接入定制；不要为了同步工程而重新初始化覆盖它。自动生成类与本机接入配置需区别对待。
- `local.properties`、SDK/NDK 路径、缓存、日志及签名文件属于本机环境，不应当作通用安装步骤复制给其他机器。
- Rust 默认输出到 `target/`；Windows MSI 默认在 `target/release/bundle/msi/`。Android Gradle APK 输出在 `crates/deskpet-app/gen/android/app/build/outputs/apk/` 下，按变体区分，以构建日志为准。
- 用户配置、数据库与运行日志不在源码目录中维护。Windows 通常为 `%APPDATA%\deskpet\`，Android 为应用私有目录，由 `paths.rs` 统一提供。

运行与打包命令见 [开发者指南](developer-guide.md)，完整资料见 [文档索引](README.md)。
