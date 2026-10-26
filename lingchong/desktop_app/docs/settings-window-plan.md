# 设置界面实现与维护说明

更新日期：2026-10-24。Windows 托盘设置与 Android 页面内设置已有实现，本文描述当前结构及剩余验收，不再将早期计划作为现状。

## 当前入口

| 平台 | 界面 | 后端 |
| --- | --- | --- |
| Windows | 托盘“设置”，frontend/settings.html、settings.css、settings.js | src/gui.rs 创建/复用窗口；ipc.rs 的 get_config / save_config、get_profile / update_profile |
| Android | 右上角齿轮，mobile.js 创建 dialog，mobile.css 布局 | ipc.rs 的 get_settings / save_settings |

所有上述路径均位于 crates/deskpet-app 内。DTO、校验与配置更新在 src/settings.rs，配置存储在 config.toml，用户画像独立存数据库。更多位置见 [项目结构](project-structure.md)。

## 已有行为

- Windows 设置包含常规、宠物、模型、采集、语音、用户画像；Android 仅模型与宠物画像。
- 桌面设置窗口关闭时隐藏，再打开复用；已显示时再次打开不会覆盖未保存编辑。
- 宠物名称、性格、表达偏好、背景设定用于后续模型回复，不自动切换动画资源。
- 密钥不回显：留空保留，输入新值替换，显式清除才删除；空向量模型可往返保存。
- 模型与宠物画像保存后用于后续请求；语音与 RSSHub 地址需重启；间隔在后续循环读取。
- Android 设置在横屏弹窗内滚动；主界面保持固定，系统键盘与小屏适配仍需设备验收。
- Windows 已有“配置已保存，画像保存或刷新未完成”的部分成功提示；配置与画像分步保存，不是原子事务。

## 待完成

Android 完整设置补齐、模型连通性诊断、语音试听/停止及播放状态、配置与画像部分失败后的统一提交/回滚方案，以及两端安装升级与真机界面回归。

源码测试与历史编译记录不代表最新安装包已交付。字段、限制与操作统一参考 [配置参考](configuration.md)、[产品使用说明书](user-manual.md)，后续工作见 [路线图](roadmap.md)。
