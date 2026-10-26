# Edge TTS 独立语音 Demo

用于 Windows 语音调试。该目录不加入 Rust workspace，不启动宠物、采集器或模型，不读取主程序的配置和密钥。

## 功能

- 输入最长 3000 字的中文或中英混合文本。
- 从在线列表选择音色，设置语速（-50～100%）、音调（-50～50Hz）。
- 合成 MP3 后用浏览器试听、暂停、停止、重播，播放音量实时可调。
- 支持取消正在进行的合成，新任务不会被旧任务结果覆盖。
- 查看合成耗时、文件大小并下载 MP3。
- 直连失败时可填写已有 HTTP 代理，合成与音色查询共用，不修改系统代理。

Demo 用浏览器播放 MP3，因此不需要 mpv，也不需要 Windows 安装中文系统音色。正式桌宠集成仍按 [Edge TTS Windows 设计](../../docs/edge-tts-windows-design.md) 使用 mpv；本 Demo 优先验证在线合成与声音参数。

## 启动

需要标准 Windows Python 3.10+（包含 pip/venv）、可联网环境，以及带音频播放能力的现代浏览器。MSYS Python 不作为推荐的 Windows 启动环境。首次安装依赖需要联网。

在 `desktop_app` 项目根目录的 PowerShell 执行：

```powershell
.\demos\edge-tts\start.ps1
```

脚本首次在 Demo 目录创建 `.venv`，安装 `requirements.txt` 中固定版本的依赖，然后启动本地服务。它不会修改主程序依赖。

浏览器打开 [http://127.0.0.1:8765](http://127.0.0.1:8765)。点击“合成并试听”才会发送文字并合成；点击“刷新在线音色”只查询音色列表。浏览器禁止自动播放时，点击音频播放器的播放按钮。

已有虚拟环境但缺少依赖时：

```powershell
.\demos\edge-tts\start.ps1 -InstallDependencies
```

更换端口：

```powershell
.\demos\edge-tts\start.ps1 -Port 8766
```

如果系统的 PowerShell 策略阻止脚本，可直接执行以下命令，无需修改系统策略：

```powershell
cd demos\edge-tts
py -3 -m venv .venv
.\.venv\Scripts\python.exe -m pip install -r requirements.txt
.\.venv\Scripts\python.exe server.py --port 8765
```

终端按 `Ctrl+C` 停止服务并取消未完成合成。浏览器已经加载的音频由浏览器播放，关闭服务后若仍在播放，可点击播放器停止或关闭页面。

## 数据和限制

- 待朗读文字发送到 Edge 在线语音服务，不需要模型 API Key。不要输入不希望发送的私人内容。
- 成功的音频保存在本目录 `output/`，方便离线重播；该目录已忽略版本控制。关闭服务并停止浏览器播放后，可手动删除不需要的文件。
- 失败或取消的 `.part` 文件会清理；Demo 不自动删除已成功生成的 MP3。
- 离线无法合成新音频，服务不可用、音色失效时显示错误，不返回虚假成功。
- 单次合成最多等待 90 秒；Demo 不自动重试。
- 播放音量由浏览器控制，不会改变下载 MP3 本身的音量。
- 多个页面共享一个后端合成任务，适合单人调试；新页面提交会替换旧合成。
- 本地服务只监听 `127.0.0.1`。不要通过代理将其暴露到公网。
- 示例音色并不代表已验证可用，点击刷新或实际合成确认。

## 文件结构

```text
demos/edge-tts/
├── server.py          # 本地 HTTP 服务、Edge TTS 任务和取消控制
├── requirements.txt   # 固定 edge-tts 版本
├── start.ps1          # Windows 虚拟环境与启动入口
├── test_server.py     # 不联网的参数/合成任务回归测试
├── web/
│   ├── index.html
│   ├── style.css
│   └── app.js
├── .venv/             # 本机生成，忽略版本控制
└── output/            # 合成 MP3，本机生成，忽略版本控制
```

## 验证

2026-10-24 已在本机通过既有 HTTP 代理验证：默认 39 字中文合成耗时约 2.84 秒，生成约 48.8 KB、8.328 秒 MP3；浏览器成功解码并播放到结束。直连曾返回 `ClientConnectorDNSError`，此时可填写本机已有的 HTTP 代理地址。本结果不等于所有网络环境或扬声器听感均已验收。

界面截图：[preview.png](preview.png)。

不安装依赖、不联网也可运行后端模拟测试：

```powershell
cd demos\edge-tts
python -m unittest -v test_server.py
node --check web/app.js
```

真实听音验收：合成默认中文句 → 播放 → 调整音量 → 停止 → 重播 → 下载；再切换音色/语速/音调重新合成。额外验证断网、合成中停止、空文本与超长文本。模拟测试不代表在线服务或扬声器已验收。

依赖资料：[edge-tts 上游](https://github.com/rany2/edge-tts)。
