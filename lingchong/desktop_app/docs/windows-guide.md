# DeskPet Windows 使用指导

更新日期：2026-10-24。完整操作说明见 [产品使用说明书](user-manual.md)，后续工作见 [未来扩展计划](roadmap.md)。本页以当前源码为准，不承诺已有可下载的 MSI 安装包。

## 1. 识别运行版本

| 构建方式 | 行为 |
| --- | --- |
| 启用 `tauri` feature | Windows GUI 桌宠、托盘、独立气泡、设置窗口 |
| 未启用 `tauri` feature | headless 后台模式，采集和回复输出到日志/终端，无桌宠窗口 |

两种构建的程序都可能叫 `deskpet-app.exe`，不能仅凭文件名判断。GUI 运行需要可用的 WebView2 环境；分发时使用完整打包产物，不将源码目录中的配置文件误当作用户数据配置。

## 2. 常用操作

- 双击宠物：立即联网刷新，只从本次结果中选择未播报的热点。
- 拖动宠物：调整窗口位置；在宠物上滚动滚轮可切换情绪动画。
- 左键点击托盘图标：显示或隐藏宠物。
- 右键托盘 →「设置」：编辑常规、宠物、模型、采集、语音和用户画像。
- 气泡正文：可内部上下滚动，底部反馈按钮固定；阅读时暂停自动隐藏。
- 右键托盘 →「退出」：完全退出；关闭设置窗口只会隐藏设置窗口。

当前托盘「立即播报」仍读取本地近 24 小时候选；有热点或生成自然回应时，先显示气泡，再尝试语音。需要本次联网结果时请双击宠物。这两个入口尚未统一。

## 3. 首次配置

打开设置，填写服务商提供的兼容 OpenAI 的 API 地址、API Key、模型名称。不支持 embeddings 时将向量模型留空。没有 Key 也可用真实热点的模板文案，但热点采集本身仍需联网。

密钥框留空保存会保留原 Key；只有输入新 Key 或显式勾选清除才改变它。保存后模型和宠物画像用于后续回复，Windows 音色用于下一条朗读；RSSHub 地址修改需要重启。具体生效范围见 [配置参考](configuration.md)。

首次启动请等待初始采集。采集源不可用或没有未播报热点时会有相应提示，不会不断复用上一次消息。

Windows 设置先保存配置、再保存用户画像。出现「配置已保存，画像保存或刷新未完成」时，部分修改已经生效，需按提示处理剩余错误。

## 4. 数据与日志

通常位于 `%APPDATA%\deskpet\`：

| 文件 | 用途 |
| --- | --- |
| `config.toml` | 常规、模型、宠物画像、采集和语音配置，包含 API Key |
| `deskpet.db` | 用户画像、热点、反馈、采集日志和已播报记录 |
| `deskpet.log` | 运行日志，另有轮转备份 |

程序不会优先读取 EXE 同目录下任意放置的 config.toml。环境变量 `DESKPET_LLM_KEY` 参与默认配置生成或缺失值回填，不会强制覆盖文件里已经保存的 api_key。

备份时先退出程序，再复制配置和数据库。分享配置与日志前移除密钥和私人内容；当前没有系统凭据加密存储。

## 5. Windows 语音

Windows GUI 使用 Edge TTS 在线合成与 mpv 播放，继续使用现有语音开关和音色选择，不新增试听入口。双击、托盘、自动播报及自然回应先显示气泡，再开始语音。合成与朗读期间暂停自动关闭，完成后恢复原有阅读计时；主动关闭气泡会停止对应语音。

首次从源码开发或打包前，在项目根目录运行 `scripts/prepare-windows-tts.ps1`，准备项目内便携运行时；网络需要代理时可传 `-Proxy`。Windows 平台配置自动将这些资源打包，普通用户不需要另装 Python/mpv。运行时缺失或网络失败时仅降级文字，不阻止启动。

**复制到其他电脑时，不能只复制 EXE。** 必须同时保留 EXE 同目录的 `tts` 文件夹，其中包含 `python/python.exe`、Python 库和 `mpv/mpv.exe`。出现“缺少语音运行时 python/python.exe”表示分发文件不完整，应重新解压完整便携包，或从构建电脑复制整个 `target/debug/tts`（Release 对应 `target/release/tts`）到 EXE 旁边。准备脚本在开发电脑执行，目标电脑无需安装开发环境。

Windows 音色保存后用于下一条回复，关闭语音会停止当前任务；headless 仍保留原外部命令方案。使用和配置详情见 [Edge TTS 接入说明](windows-edge-integration-plan.md)。源码集成和底层语音烟测已完成，新的 MSI 与完整桌面交互尚需验收。

## 6. 从源码运行与打包

以下命令从包含工作区 Cargo.toml 的 `desktop_app` 目录执行；需要已经可用的 Rust、相应 Windows 编译工具链及 GUI 依赖。

```powershell
# GUI 桌宠
cargo run -p deskpet-app --features tauri

# headless：没有窗口，终端 Ctrl+C 停止
cargo run -p deskpet-app

# GUI 编译检查
cargo check -p deskpet-app --features tauri
```

MSI 打包在应用 crate 目录执行：

```powershell
cd crates\deskpet-app
cargo tauri build --features tauri --bundles msi
```

也可以在项目根目录生成包含语音依赖的便携 ZIP：

```powershell
.\scripts\package-windows-portable.ps1
# 使用已经构建的 Debug EXE 制作测试包：
.\scripts\package-windows-portable.ps1 -Configuration Debug -SkipBuild
```

脚本先检查依赖，默认构建 Release GUI，再打包到 `dist/`，验证包内 Python 可以导入 Edge TTS、mpv 可以启动。分发整个 ZIP，在目标电脑完整解压后运行；目标电脑需具备 WebView2 环境及可访问 Edge TTS 的网络。打包不包含开发者配置、API Key 或数据库。

没有安装 Tauri CLI 时先按项目环境准备 `cargo install tauri-cli --version "^2" --locked`。首次打包可能需要下载 WiX 等工具；网络失败时，Rust 编译完成的 EXE 不等于 MSI 已生成。

成功后查看构建日志给出的 MSI 路径，工作区默认输出目录为 `desktop_app\target\release\bundle\msi\`。不能仅凭输出目录存在认定其中是最新安装包，需核对文件时间和构建结果。

## 7. 排障

| 现象 | 检查项 |
| --- | --- |
| 没有窗口 | 是否运行了未启用 tauri 的 headless 构建；是否有 WebView2 启动问题 |
| 没有自动播报 | 勿扰策略、当前候选、已播报记录、网络和模型调用；手动双击可帮助区分自动策略与联网问题 |
| 双击刷新失败 | 是否启用采集平台，网络或 RSSHub 是否可用，是否收到超时提示 |
| 自定义勿扰重启后未按预期工作 | 启动时策略初始化仍有已知差异，打开设置重新保存；后续计划修复，见说明书 |
| 没有声音 | 先检查本页第 5 节，确认入口是否接语音以及外部依赖是否存在 |
| 修改没有出现在程序中 | 重新构建 GUI 版本，并确认启动了新产物；更新不会自动覆盖用户保存的配置 |

日志可能含模型请求、响应及画像内容，向他人提供前请检查并脱敏。
