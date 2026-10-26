# Android 路线与可行性说明

更新日期：2026-10-24。早期报告评估的是“是否移植”；现在已有 Android 工程和前台界面，本文改为说明当前选型与剩余边界，不再使用未经测量的代码复用百分比。

## 当前结论

项目已选择 Tauri 2 Android：复用 Rust 核心业务，单 Activity / WebView 承载界面。Android 面向实体桌宠头部的横向屏幕，在应用前台提供陪伴展示与热点气泡。它与 Windows 透明桌面窗口属于不同呈现形态。

| 层次 | 当前方案 | 主要约束 |
| --- | --- | --- |
| 类型、存储、采集、评分与回复 | 复用 core / storage / collector / scheduler / agent | SDK/NDK 交叉编译、网络与来源限制 |
| 应用入口与路径 | lib.rs 移动端入口，paths.rs 注入私有目录 | 初始化顺序、配置与数据库生命周期 |
| 界面 | 共用静态宠物资源，mobile.js/css 适配 Android | 屏幕尺寸、WebView、触摸与系统栏 |
| 原生 Activity | Manifest 横屏，MainActivity 沉浸与前台亮屏 | 实际设备仍需验收 |
| 语音 | 现有桌面外部命令不能作为 Android 可用方案 | 原生语音适配尚未完成 |
| 后台 | 当前以前台运行作为使用边界 | 无前台服务、系统调度或后台可靠性保证 |

## 实体桌宠的含义

机身头部是一块运行本软件的 Android 屏幕。当前实现的是屏幕软件和外观概念，不包含传感器、马达、硬件控制、机械结构或量产设计。屏幕前台亮起不意味着系统级 kiosk、设备管理或自动恢复应用已经实现。

## 后续判断条件

- 先用目标屏幕验收可读性、操作面积、持续运行及温升，再决定是否增加更多画面内容。
- 离开前台持续采集/语音需要独立的生命周期与系统集成设计。
- 语音功能需要明确引擎、播放、停止、错误与开关行为，不能依赖桌面上的 Python 命令和随机存在的播放器。
- 不因已有工程目录或旧 APK 就宣称当前版本安装、升级和全部功能验证完成。

资料：[项目结构](project-structure.md)、[Android 移植计划](android-porting-plan.md)、[Android 工程指南](android-studio.md)、[实体桌宠概念](design/desktop-companion.md)、[当前实现](../implementation-status.md)。
