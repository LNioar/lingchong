# Android 移植进度与后续计划

更新日期：2026-10-24。本文替代早期“竖屏卡片 / 通知优先”的阶段描述，以当前已有 Android 工程和前台屏幕桌宠形态为基线。

## 当前路线与目录

采用 Tauri 2 Android，复用 Rust 业务 crate，以单 Activity / WebView 显示宠物。项目根目录是 desktop_app，Android 工程为 crates/deskpet-app/gen/android，界面源文件在 crates/deskpet-app/frontend。完整结构见 [项目结构](project-structure.md)，构建见 [开发者指南](developer-guide.md) 和 [Android 工程指南](android-studio.md)。

## 已落地

| 工作 | 源码状态 |
| --- | --- |
| 移动端入口 | lib.rs 共享库与 Tauri mobile_entry_point |
| 数据路径 | paths.rs，GUI 启动早期注入应用私有目录 |
| 平台分支 | gui.rs 隔离桌面托盘、气泡和设置窗口 |
| Android 工程 | Gradle、buildSrc、应用模块与 MainActivity 均已存在 |
| 静态前端 | custom-protocol 加载 APK 内资源 |
| 屏幕形态 | Manifest 声明横屏；系统栏沉浸隐藏；Activity 前台保持亮屏 |
| 触摸与回复 | 双击实时联网刷新、重复请求保护、模型/模板回复、临时情绪恢复 |
| 桌宠 UI | 深色待机界面、宠物居中；消息时左侧宠物、右上角内部滚动气泡 |
| 设置与反馈 | 模型与宠物画像 dialog；喜欢、不感兴趣、关闭 |
| 按钮状态 | “听点新鲜事”已隐藏，操作提示仅引导双击 |

上述是源码状态，不能据此声称新 APK 已完成实机验收。此前构建记录和测试适用范围见 Android 工程指南。

## 后续工作

1. 在目标头部屏幕上验收横屏、比例、触摸、长气泡、设置键盘遮挡和恢复前台行为；覆盖安装确认数据保留。
2. 接入 Android 原生语音及可观察的播放状态；双击文本、语音失败与停止操作形成闭环。
3. 补齐采集、勿扰、用户画像等设置，保持与 Windows 后端配置语义一致。
4. 如确有离开前台继续工作需求，再设计前台服务 / 系统调度、通知和用户控制；不能将当前 Tokio 循环视为已完成后台方案。
5. 确定机身与屏幕规格后再开展支架、供电、散热与结构工程；当前外观图不是 CAD 或可生产模型。

系统悬浮窗、语音输入、运动控制、硬件传感器和开机启动未因本次界面改版而实现。完整优先顺序与验收条件见 [未来扩展计划](roadmap.md)，外观与屏幕设计见 [实体桌宠设计](design/desktop-companion.md)。
