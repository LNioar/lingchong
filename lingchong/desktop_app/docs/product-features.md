# DeskPet 产品功能说明

核对日期：2026-10-24。本文对照当前源码整理产品能力；操作步骤见 [用户使用说明书](user-manual.md)。已实现指代码存在，不代表最新 MSI / APK 已完成安装与设备验收。

## 产品形态

Windows 提供透明桌宠窗口、独立气泡和托盘设置；Android 提供用于实体桌宠头部屏幕的前台横屏界面。Android 外观概念不包含已实现的机械、传感器、语音输入或系统悬浮窗。

## 当前功能与源码依据

| 功能 | 当前行为 | 源码依据 |
| --- | --- | --- |
| 热点采集 | 知乎、B站、微博及 RSSHub 相关路由，结果受网络与来源限制 | [采集管理](../crates/deskpet-collector/src/manager.rs) |
| 手动获取热点 | 双击宠物，刷新启用来源，只从本次返回的未播报热点中选取；错误不复用旧新闻 | [IPC](../crates/deskpet-app/src/ipc.rs)、[运行时](../crates/deskpet-app/src/runtime.rs) |
| 无热点回应 | 手动请求没有可播报结果时，按宠物和主人画像生成一句回应；无 Key 或失败时用本地文案，不编造新闻和链接 | [Agent 编排](../crates/deskpet-agent/src/orchestrator.rs) |
| 自动播报 | 按勿扰、间隔与评分策略选择内容，空结果保持安静；启动时自定义策略存在初始化限制 | [调度器](../crates/deskpet-scheduler/src/scheduler.rs)、[运行时](../crates/deskpet-app/src/runtime.rs) |
| 内容选择 | 无兴趣条目门槛 0.3；有兴趣但没有可用向量时跳过相似度并将配置门槛乘 0.4；手动请求跳过评分门槛 | [评分](../crates/deskpet-agent/src/scoring.rs)、[Agent 编排](../crates/deskpet-agent/src/orchestrator.rs) |
| 模型与模板 | 配置 Key 后生成宠物口吻回复；未配置时用真实热点模板；当前回复不发送 tools | [模型客户端](../crates/deskpet-agent/src/llm.rs)、[Agent 编排](../crates/deskpet-agent/src/orchestrator.rs) |
| 去重与记录 | 按热点 ID 保存已播报记录；有向量与语义去重基础，仍不保证同事件跨源只出现一次 | [数据库结构](../crates/deskpet-storage/src/schema.rs)、[去重](../crates/deskpet-collector/src/dedup.rs) |
| 兴趣反馈 | 喜欢、不感兴趣、关闭；Windows 另有原文点击和自动隐藏反馈，按热点标签调整兴趣 | [IPC](../crates/deskpet-app/src/ipc.rs)、[桌面气泡](../crates/deskpet-app/frontend/bubble.html) |
| 宠物画像 | 名称、性格、表达偏好与背景用于后续模型回复；不更换动画或音色 | [设置校验](../crates/deskpet-app/src/settings.rs)、[Agent 编排](../crates/deskpet-agent/src/orchestrator.rs) |
| 动画 | 六种情绪、临时情绪恢复和 SVG 后备；查询与无热点回应可发送 excited，恢复行为有平台差异 | [宠物页面](../crates/deskpet-app/frontend/index.html)、[移动端](../crates/deskpet-app/frontend/mobile.js) |
| Windows 界面 | 拖动宠物、托盘显示/隐藏/设置/退出、滚动气泡、阅读暂停自动关闭 | [GUI](../crates/deskpet-app/src/gui.rs)、[气泡](../crates/deskpet-app/frontend/bubble.html) |
| Android 界面 | 横屏、沉浸全屏、前台亮屏；待机居中，消息时宠物左侧、气泡右上方；单击热点按钮隐藏 | [移动端布局](../crates/deskpet-app/frontend/mobile.css)、[交互](../crates/deskpet-app/frontend/mobile.js)、[Manifest](../crates/deskpet-app/gen/android/app/src/main/AndroidManifest.xml)、[Activity](../crates/deskpet-app/gen/android/app/src/main/java/com/deskpet/app/MainActivity.kt) |
| 设置与保存 | Windows 完整设置，Android 模型与宠物设置；Key 留空保留、显式清除；模型、人设与 Windows 音色用于后续回复，关闭 Windows 语音停止当前任务，RSSHub 地址需重启 | [桌面设置](../crates/deskpet-app/frontend/settings.js)、[设置后端](../crates/deskpet-app/src/settings.rs)、[重载](../crates/deskpet-app/src/runtime.rs) |
| 数据与日志 | 本地配置、SQLite 和轮转日志；没有账号云同步或 Android 应用内导入导出 | [路径](../crates/deskpet-app/src/paths.rs)、[日志](../crates/deskpet-app/src/logging.rs) |

## 已知限制

- 双击刷新与热点生成阶段约 45 秒超时；空结果自然回应在该计时之外，整个操作可能等待更久。
- Windows 托盘“立即播报”仍选本地近 24 小时候选，与双击联网入口不同；语音接入未改变候选来源。
- Windows GUI 已接入 Edge 在线合成与 mpv：先显示气泡，语音期间保持，主动关闭停止；继续使用现有音色与开关，不增加试听。依赖准备与完整验收范围见 [接入说明](windows-edge-integration-plan.md)。Android 原生 TTS 尚未实现，headless 保留旧路径。
- Android 无后台可靠运行保证、系统悬浮宠物或开机启动；前台亮屏不等于后台保活。
- Windows 配置与用户画像分步保存，已有部分成功提示，未实现统一原子提交；保存成功不等于模型服务连通性检查成功。

## 本次核对范围

本次核对了前端事件与布局、IPC、运行时、配置与设置、Agent 回复和评分、语音实现及 Android Activity。只更新文档，不改变程序行为，不重新构建安装包。链接、编码与代码块由 `node scripts/check-docs.cjs` 检查；真实设备效果与升级行为仍需验收。

当前目录见 [项目结构](project-structure.md)，全部资料见 [文档索引](README.md)。
