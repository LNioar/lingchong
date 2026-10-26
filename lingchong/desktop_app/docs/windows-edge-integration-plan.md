# Windows Edge TTS 主项目接入

更新日期：2026-10-24。状态：已完成源码接入；完整 GUI 整机验收边界见下文。

## 已采纳的分步计划

| 步骤 | 内容 | 当前状态 |
| --- | --- | --- |
| 1 | 复用现有语音开关与音色，不增加试听入口 | 已实现 |
| 2 | 气泡显示确认后再合成和播放，语音过程暂停自动关闭 | 已实现 |
| 3 | 主动关闭或替换气泡停止对应任务，过期事件不影响新消息 | 已实现 |
| 4 | 接入双击、自然回应、托盘与自动播报，验证平台和配置兼容 | 已接入并完成下列检查 |

本次用户要求取代早期“先增加设置试听、默认另设后端开关”的建议。Windows GUI 使用现有 `tts.enabled` 和 `tts.voice`，无需增加第二个语音开关；Android 与 headless 保留原语音路径。

## 使用行为

1. 真实热点回复或自然回应先显示气泡。刷新中、失败提示等固定状态不朗读。
2. Windows 为气泡分配唯一 `bubble_id`，前端两次动画帧后确认显示，后端才开始合成。
3. 合成与播放期间不会因原有 15 秒计时自动隐藏气泡；用户仍可主动关闭。
4. 语音完成后重新开始原有 15 秒阅读计时；鼠标停留、焦点和滚动阅读的暂停规则保留。不是说完立即关闭。
5. 关闭、喜欢、不感兴趣、查看原文等原本会隐藏气泡的操作，同时取消关联语音，保留原反馈语义。
6. 新消息替换旧气泡时取消旧任务，新任务在旧进程回收后运行，避免叠音。
7. 合成、播放或依赖失败时保留文字，显示语音错误并恢复正常关闭计时，不重试热点采集或写重复播报记录。
8. 关闭现有语音开关并保存后，当前语音停止；修改音色供下一条回复使用。
9. 程序退出时取消任务；Windows Job Object 随进程关闭会终止其受管子进程，避免退出后继续发声。
10. 语音遇到首个 `http://` 或 `https://` 链接就结束，忽略大小写，链接及其后文不朗读；气泡仍保留完整原文。开头就是链接时不启动语音任务，气泡使用正常阅读计时。

## 代码位置

| 位置 | 职责 |
| --- | --- |
| [windows_speech.rs](../crates/deskpet-app/src/windows_speech.rs) | Windows GUI 语音任务、显示确认、状态关联、资源路径与临时文件 |
| [windows_process.rs](../crates/deskpet-tts/src/windows_process.rs) | 可独立测试的 Windows 进程隔离、停止、禁用与超时 |
| [edge_worker.py](../crates/deskpet-app/resources/tts/edge_worker.py) | JSON 标准输入、Edge 合成、系统代理及取消后的文件处理 |
| [gui.rs](../crates/deskpet-app/src/gui.rs)、[ipc.rs](../crates/deskpet-app/src/ipc.rs) | 接入各回复入口、气泡显示确认及关联关闭 |
| [bubble.html](../crates/deskpet-app/frontend/bubble.html) | 语音状态、暂停自动关闭、拒绝过期事件和显示确认 |
| [config.rs](../crates/deskpet-app/src/config.rs)、[settings.rs](../crates/deskpet-app/src/settings.rs) | 追加可选运行时配置，兼容旧配置和旧设置页面 |
| [tauri.windows.conf.json](../crates/deskpet-app/tauri.windows.conf.json) | 仅 Windows 打包便携语音资源 |
| [prepare-windows-tts.ps1](../scripts/prepare-windows-tts.ps1) | 开发时显式准备依赖，应用启动时不安装依赖 |

主项目不启动 Demo HTTP 服务，不使用端口 8765，不修改采集器、Agent 打分、SQLite schema 或已播报记录规则，也不新增试听或播放按钮。

## 开发与打包

当前准备脚本使用 Demo 的独立 Python 3.13 环境作为构建工具，生成的主程序运行时是独立的便携 Python，运行不依赖 Demo 服务。先按 [Demo 文档](../demos/edge-tts/README.md) 准备构建工具，再从项目根目录执行：

```powershell
.\scripts\prepare-windows-tts.ps1
# 网络需要代理时使用已存在的代理，例如：
.\scripts\prepare-windows-tts.ps1 -Proxy http://127.0.0.1:7890

cargo run -p deskpet-app --features tauri
```

代理地址只是示例，不硬编码到产品。合成 worker 默认读取系统 HTTP/HTTPS 代理；显式配置为空字符串时直连。

Windows x64 依赖固定为 CPython 3.13.14、edge-tts 7.3.1、mpv 20261021 x86_64 构建；Python 与默认 mpv 压缩包校验 SHA-256。生成的运行时和音频不提交版本控制，许可文件随依赖保留。Windows 目录配置由 Tauri 自动合并，Android 不包含这些资源。

打包前完成依赖准备，然后执行：

```powershell
cd crates\deskpet-app
cargo tauri build --features tauri --bundles msi
```

干净源码 checkout 只带资源目录占位说明，可编译和启动文字界面；要让语音实际工作，需准备运行时后重新构建。缺少依赖不会使宠物启动失败。

跨电脑分发不能单独复制 EXE。可在项目根目录执行 [package-windows-portable.ps1](../scripts/package-windows-portable.ps1)，默认构建 Release 并生成包含 `tts/python`、`tts/mpv` 的便携 ZIP；已有 Debug 构建可传 `-Configuration Debug -SkipBuild`。脚本检查依赖并运行包内 Python 导入与 mpv 启动验证。目标电脑完整解压 ZIP，无需另装 Python/mpv。手工修复已有 EXE 时，将对应构建目录下整个 `tts` 文件夹复制到 EXE 同目录。

## 可选的运行时配置

继续使用现有语音开关、provider 和音色。需要指定路径或代理时，可手工追加：

```toml
[tts.edge]
python_path = "C:/portable/python/python.exe"
mpv_path = "C:/portable/mpv/mpv.exe"
proxy = "http://127.0.0.1:7890"
```

也可通过 `DESKPET_TTS_PYTHON` 和 `DESKPET_TTS_MPV` 指定可执行文件。优先级：配置显式路径 → 环境变量 → 应用资源目录 → 仅 Debug 下的项目内运行时。实际发行包不引用开发机目录。

没有 `[tts.edge]` 的旧配置正常读取；旧设置页面只提交原有字段时，保留已存在的运行时配置。保存普通设置不会清空运行时路径或代理。

## 验证记录

- 宠物交互回归通过，双击、防重复触发和情绪计时保持。
- 气泡回归通过：长文滚动、阅读暂停、合成/播放不超时关闭、结束后恢复计时、主动关闭传任务 ID、过期事件和迟到缓存不覆盖新消息。
- 10 项配置/设置测试通过，包括旧配置、密钥保留、向量模型为空、原子校验及语音运行时参数保留。
- Windows 进程控制测试通过：正常/错误退出、取消、关闭语音、超时及提前取消。
- 已使用准备的便携 Python、Edge TTS 和 mpv 完成在线中文合成、完整播放和中途停止测试。
- Windows GUI 源码编译检查、可执行文件构建与 Android ARM64 交叉编译检查通过；headless 编译检查通过。

完整 GUI 测试二进制在本机 GNU 工具链中出现 `STATUS_ENTRYPOINT_NOT_FOUND`，MSVC 工具链缺少 `link.exe`。这些检查不能替代完整宠物窗口的实际交互与新 MSI 安装验收；尚未声称完成整机与安装包验收。Windows GNU 的共享库导出数量问题另通过仅 Windows GNU 生效的 cdylib 链接选项处理，不改变 Android 的移动入口导出。

另增加了仅 Debug 可用的 `--speech-smoke-test`，数据写入 `target/tts-gui-smoke/`，只使用固定句子，不启动采集或模型请求。成功时应写出 `result.json`，不能仅按进程退出码判断。本机隔离 GUI 检查没有生成该完成记录，未计入通过项。普通启动与 Release 不进入此测试分支。

后续整机验收应至少检查：双击真实回复、播报超过 15 秒、合成中关闭、播放中关闭、新旧消息交替、关闭语音、退出无残留声音、覆盖安装与断网文字降级。
