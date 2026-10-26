# DeskPet 文档索引

更新日期：2026-10-24。有效项目根目录为 `desktop_app/`，同级其他备份目录不属于本项目维护范围。

## 使用与配置

| 文档 | 用途 |
| --- | --- |
| [产品功能说明](product-features.md) | 当前功能、源码依据、平台差异和已知限制 |
| [产品使用说明书](user-manual.md) | Windows / Android 当前操作与能力边界 |
| [配置参考](configuration.md) | 配置字段、保存与热更新范围 |
| [Windows 指南](windows-guide.md) | Windows 运行、打包、数据与排障 |
| [实体桌宠设计](design/desktop-companion.md) | Android 屏幕头部概念、界面及验收边界 |

## 开发与项目现状

| 文档 | 用途 |
| --- | --- |
| [项目结构](project-structure.md) | 当前目录、源码入口、资源与生成文件边界 |
| [开发者指南](developer-guide.md) | 从正确工作目录执行运行、测试和打包 |
| [架构设计](architecture.md) | 模块职责、平台分支、数据与事件流 |
| [实现状态](../implementation-status.md) | 当前源码能力、未完成项和验证范围 |
| [Android 工程指南](android-studio.md) | 当前入口、构建配置及历史排障记录 |
| [Android 移植计划](android-porting-plan.md) | 已落地能力与剩余 Android 工作 |
| [Android 可行性说明](android-feasibility-report.md) | 已选择路线及持续存在的技术边界 |
| [设置界面说明](settings-window-plan.md) | 当前设置页面、IPC 与后续工作 |
| [未来扩展计划](roadmap.md) | 规划功能与验收条件 |
| [语音输出实施计划](voice-output-plan.md) | 待实施的 Android 优先语音交付计划 |
| [Windows Edge TTS 设计](edge-tts-windows-design.md) | 在线合成、mpv 播放与正式集成方案 |
| [Edge TTS 主项目接入](windows-edge-integration-plan.md) | Windows 隔离、渐进接入、兼容与回归方案 |
| [Edge TTS 独立 Demo](../demos/edge-tts/README.md) | 已实现的本地网页语音调试；浏览器播放，不启动主程序 |
| [Token 消耗分析](token-analysis.md) | 当前请求路径与成本估算方法，不是实际账单 |
| [精灵图说明](../crates/deskpet-app/frontend/sprites/README.md) | 动画文件与配置 |

## 阅读约定

当前源码是实现依据；历史构建记录只能证明当时的产物。目录存在、根目录有 MSI 或工程已有 APK，都不能证明它们包含最新修改。功能更新需要重新构建并安装。

`diagnostics/` 是诊断快照，`.tools/` 是被忽略的本地辅助目录，`target/` 和 Android `build/` 是生成产物；它们不替代本索引中的维护文档。
