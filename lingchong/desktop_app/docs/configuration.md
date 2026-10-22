# 配置参考

更新日期：2026-10-22。操作说明见 [产品使用说明书](user-manual.md)。本文件描述当前源码；设置界面并不代表相应依赖或服务已经验证可用。

## 1. 配置位置与入口

| 平台 | 配置位置 / 入口 |
| --- | --- |
| Windows | `%APPDATA%\deskpet\config.toml`；右键托盘 →「设置」 |
| Android | 应用私有数据目录；页面右上角 →「设置」，当前仅模型与宠物画像 |
| Linux / macOS | 由系统数据目录定位的 deskpet 子目录；不在本轮平台验收范围 |

首次运行创建默认配置。不要将 EXE 同目录或仓库内的 config.toml 当成当前生效文件。Windows 用户画像、热点和反馈存于同目录的 deskpet.db，而非 config.toml。

## 2. 保存、热更新与重启

| 字段 | 生效范围 |
| --- | --- |
| `llm.*` | 保存后重建对话/向量提供方，供后续请求使用 |
| `pet.*` | 保存后更新 Agent 宠物画像，下一次模型回复使用 |
| `general.quiet_hours` / `announce_threshold` | 保存后更新运行时策略 / 阈值 |
| `general.min_announce_interval_min` | 更新运行时，已有循环等待可能仍按旧间隔结束 |
| `fetch.platforms[].enabled` | 后续定时和手动刷新读取，不取消已发出的请求 |
| `fetch.platforms[].interval_min` | 后续定时循环读取；不限制显式双击刷新 |
| `fetch.default_interval_min` | 启动时缺少单个平台间隔配置时的默认值，完整默认配置已有各平台间隔 |
| `fetch.rsshub_endpoint` | 需要重启，Adapter 启动时构建 |
| `tts.*` | 需要重启，TTS 实例启动时构建 |

设置界面保存会立即调用重载；手工修改文件由监听器约每 5 秒检查变化。保存不代表远端服务鉴权、模型或音色连通性测试成功。

已知限制：启动时打扰策略尚未完整从磁盘同步，可能先使用默认勿扰/最小间隔；启动后再次保存设置可应用自定义策略。已开始的等待也不会立即重置。后续修复计划见 [roadmap](roadmap.md)。

## 3. 配置示例

以下是结构示例，模型名称和地址必须按实际服务商填写；Key 留空表示使用基础模板模式。

```toml
[general]
quiet_hours = ["23:00-08:00"]
min_announce_interval_min = 45
announce_threshold = 0.6

[pet]
name = "基尔兽"
personality = "活泼、好奇、友善"
speaking_style = "自然亲切，有自己的想法，表达方式灵活"
background = ""

[fetch]
default_interval_min = 15
rsshub_endpoint = "https://rsshub.app"

[[fetch.platforms]]
id = "zhihu"
enabled = true
interval_min = 10

[[fetch.platforms]]
id = "bilibili"
enabled = true
interval_min = 15

[[fetch.platforms]]
id = "weibo"
enabled = true
interval_min = 20

[[fetch.platforms]]
id = "rsshub"
enabled = true
interval_min = 30

[[fetch.platforms]]
id = "douyin"
enabled = true
interval_min = 30

[[fetch.platforms]]
id = "xiaohongshu"
enabled = true
interval_min = 30

[llm]
api_base = "https://api.openai.com/v1"
api_key = ""
model = "gpt-4o-mini"
# 不支持 embeddings 时可不写此键；设置界面留空保存会写为 None。
# embedding_model = "text-embedding-3-small"

[tts]
provider = "edge"
voice = "zh-CN-XiaoxiaoNeural"
enabled = true
```

## 4. 字段说明

### `[general]`

| 字段 | 默认值 | 说明 |
| --- | --- | --- |
| `quiet_hours` | `["23:00-08:00"]` | 勿扰时段数组，HH:MM-HH:MM，支持跨夜；空数组关闭勿扰时段 |
| `min_announce_interval_min` | `45` | 至少 1 分钟，自动播报的最小间隔 |
| `announce_threshold` | `0.6` | 0～1；有兴趣画像时用于自动播报筛选，无兴趣使用较低冷启动门槛 |

双击手动刷新跳过自动播报门槛与勿扰检查，不因此取消“不感兴趣”和已播报排除。

### `[pet]`

| 字段 | 默认值 | 校验 / 用途 |
| --- | --- | --- |
| `name` | `基尔兽` | 不能为空，最多 80 字符 |
| `personality` | `活泼、好奇、友善` | 性格描述，最多 4000 字符 |
| `speaking_style` | `自然亲切，有自己的想法，表达方式灵活` | 表达偏好，最多 4000 字符 |
| `background` | 空 | 背景设定，最多 4000 字符 |

旧配置缺少 pet 节会加载默认画像；修改画像影响后续模型提示，不替换动画图片或 TTS 音色。用户画像是另一份数据，通过设置中的用户画像分区及热点反馈修改。

### `[fetch]` 与 `[[fetch.platforms]]`

| 字段 | 默认值 / 规则 | 说明 |
| --- | --- | --- |
| `default_interval_min` | `15`，至少 1 | 启动默认间隔 |
| `rsshub_endpoint` | `https://rsshub.app` | HTTP/HTTPS RSSHub 地址，需要重启 |
| `platforms[].id` | 不可重复 | zhihu / bilibili / weibo / rsshub / douyin / xiaohongshu |
| `platforms[].enabled` | 默认全部 true | 后续请求启用开关 |
| `platforms[].interval_min` | 至少 1 | 默认知乎 10、B站 15、微博 20、其余 30 分钟 |

抖音、小红书当前经 RSSHub 路由采集。平台开关与 RSSHub 聚合、回退路由仍有整合空间，不承诺单个平台开关能排除聚合源内所有同平台内容。

### `[llm]`

| 字段 | 默认值 / 规则 | 说明 |
| --- | --- | --- |
| `api_base` | `https://api.openai.com/v1` | 设置界面校验 HTTP/HTTPS 地址，不允许 URL 账号密码、查询参数和片段 |
| `api_key` | 空或默认生成时的环境变量值 | 未配置则不发起对话请求，使用模板 |
| `model` | `gpt-4o-mini` | 必填，使用服务商支持的实际模型标识 |
| `embedding_model` | 新建默认配置预设 `text-embedding-3-small`；文件缺失该键时为 None | 留空或 None 禁用向量模型，持久化后不自动回填 |

向量功能同时要求非空 Key 和非空向量模型名。无真实向量时使用哈希占位向量，不等同于语义匹配。

密钥设置要区分两种入口：

- 设置界面输入留空会保留已保存 Key，显式输入新值替换，勾选清除后保存才删除。
- 直接编辑 TOML 的 `api_key = ""` 则表示清空。

`DESKPET_LLM_KEY` 只参与默认构造或缺失字段的回填，不会无条件覆盖配置文件中已存在的 api_key。优先使用应用设置，避免误以为环境变量始终优先。

### `[tts]`

| 字段 | 默认值 | 当前实现 |
| --- | --- | --- |
| `provider` | `edge` | 当前运行时固定构造 EdgeTts，尚未实现按 provider 动态切换引擎 |
| `voice` | `zh-CN-XiaoxiaoNeural` | 传递给外部 edge-tts 的音色 ID，重启后使用 |
| `enabled` | `true` | 启动时决定是否创建语音实例，开启不等于依赖已就绪 |

当前语音依赖外部合成命令与播放器；自动/托盘/headless 有调用，双击未调用，Android 未接入原生语音。完整状态见 [说明书语音章节](user-manual.md#8-语音功能的真实状态)。

## 5. 诊断与数据安全

保存成功与服务可用是两回事。当前无完整模型连通性、音色可用性测试或播放状态界面。

配置文件包含明文密钥，日志可能包含请求/响应和画像。分享前脱敏；备份前先退出程序，保留配置和数据库，不要仅复制可执行文件。