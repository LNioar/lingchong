# DeskPet 托盘设置界面 — 实施计划

> 目标：系统托盘右键菜单新增「设置」，打开设置窗口，支持配置勿扰时间、用户画像、API Key 等。
> 状态：**已实现，尚待实机界面回归**。
> 范围：Windows 托盘设置窗口；Android 使用页面内模型与宠物设置，另已增加沉浸式全屏。

## 本次补充：气泡滚动与宠物画像

- Windows 气泡取消四行正文裁剪，标题、正文和原文链接放入可滚动区域，底部反馈按钮固定保留。内容支持鼠标滚轮和键盘滚动；新气泡从顶部显示，鼠标停留或键盘焦点在气泡中时暂停自动关闭，离开后重新计时。
- Windows 设置新增独立的「宠物画像」，Android 设置同步增加名称、性格、表达偏好、背景设定。宠物画像保存到配置文件的 `[pet]`，与数据库中的用户画像独立。
- 保存后热更新 Agent，下一次模型回复会使用宠物画像，不增加固定回复格式或示例；未配置模型时仍使用基础模板。旧配置缺少 `[pet]` 会获得默认画像，旧客户端不传 pet 字段会保留现有画像。
- 保留向量模型为空时的往返修复，并以测试验证宠物画像保存不会使其回填默认值。
- 验证：Windows GUI 编译检查通过，18 项 Agent 测试、8 项设置/配置兼容测试以及气泡/宠物前端逻辑回归通过。已知受沙箱文件重命名限制的持久化文件测试未重跑；未生成本轮安装包，尚未完成实机界面回归。

本次验证命令：

```powershell
cargo check -p deskpet-app --features tauri --offline
cargo test -p deskpet-agent --offline
cargo test -p deskpet-app --lib --offline -- --skip saving_replaces_existing_config_and_round_trips_key
node scripts/test-bubble-ui.cjs
node scripts/test-pet-ui.cjs
```

## 实施记录（2026-10-21）

- 托盘新增「设置」，懒创建普通窗口，关闭时隐藏，再打开复用并重载配置；窗口已打开时再次点击仅前置，不覆盖未保存编辑。
- 设置页面包含常规、模型、六个采集平台、语音、用户画像五个分区，对应 `settings.html` / `settings.css` / `settings.js`。
- 新增 `get_config` / `save_config`，沿用 `get_profile` / `update_profile`。后端统一验证时间、数值、地址与平台；修复画像 IPC 在异步线程中使用 blocking_lock 的问题，数据库失败会返回错误。
- 相比下方原始设计，API Key 不返回任何前后缀；只返回 `has_api_key`，空输入保留原值，并提供显式清除选项。Android 的 `get_settings` / `save_settings` 保持兼容。
- 平台开关现在同时约束启动采集和定时采集；关闭后不会再发起新的该平台采集，正在执行的请求不取消。间隔在下一次定时循环读取，语音和 RSSHub 地址更改需重启，页面已注明。
- 另调整 `capabilities/default.json`，允许 settings 窗口访问事件等核心能力；无需修改 tauri.conf.json。
- Android 在 MainActivity 使用 WindowInsetsControllerCompat 隐藏 systemBars，并在 onResume / 获得焦点时重新应用；边缘滑动可临时显示系统栏。
- 验证：Windows GUI cargo check、设置模块 4 项单元测试及前端 JavaScript 语法检查通过。配置文件重命名测试被沙箱限制，沙箱外测试授权被拒绝；Android Kotlin 编译同样因沙箱原生库限制及提权授权被拒绝而未完成。尚未生成本轮安装包或完成两端实机回归。

以下保留原始实施计划供设计对照；与上述记录不一致之处，以当前实现为准。

---

## 1. 现状确认

| 关注点 | 现状 | 文件 |
| --- | --- | --- |
| 托盘菜单 | `显示宠物 / 隐藏宠物 / 立即播报 / 退出`，事件在 `on_menu_event` | `crates/deskpet-app/src/gui.rs` |
| 配置结构 | `AppConfig { general, fetch, llm, tts }`，TOML 存于 `data_dir()/config.toml` | `crates/deskpet-app/src/config.rs` |
| 配置读写 | `AppConfig::load()` / `AppConfig::save()` 已存在 | 同上 |
| 热更新 | `reload_config()` 支持勿扰时段、播报间隔、采集间隔、阈值、LLM；`start_config_watcher()` 每 5s 轮询 mtime 自动重载 | `crates/deskpet-app/src/runtime.rs` |
| 不可热更 | TTS 实例（需重启） | 同上 |
| 画像读写 | IPC `get_profile` / `update_profile` 已有 | `crates/deskpet-app/src/ipc.rs` |
| 窗口创建 | 全部运行期创建；`tauri.conf.json` 无 `app.windows` | `gui.rs` / `tauri.conf.json` |
| 前端 | `frontendDist: "frontend"`，`withGlobalTauri: true`，可直接用 `window.__TAURI__` | `tauri.conf.json` |

### 关键配置字段（当前真实结构）

```toml
[general]
quiet_hours = ["23:00-08:00"]   # Vec<String>，格式 HH:MM-HH:MM
min_announce_interval_min = 45
announce_threshold = 0.6

[fetch]
default_interval_min = 15
rsshub_endpoint = "https://rsshub.app"
# platforms: id / enabled / interval_min
# zhihu, bilibili, weibo, rsshub, douyin, xiaohongshu

[llm]
api_base = "https://api.openai.com/v1"
api_key = ""
model = "gpt-4o-mini"
embedding_model = "text-embedding-3-small"   # 置空 = 关闭 embedding

[tts]
provider = "edge"
voice = "zh-CN-XiaoxiaoNeural"
enabled = true
```

用户画像 `UserProfile { interests: Vec<Interest>, persona: String, schedule: SchedulePref, updated_at }`，
其中 `Interest { topic, weight(0~1), last_hit, embedding }`。

---

## 2. 改动清单

| # | 文件 | 改动 |
| --- | --- | --- |
| 1 | `crates/deskpet-app/src/gui.rs` | 托盘菜单项 + 事件分支 + `ensure_settings_window` + 注册新 IPC |
| 2 | `crates/deskpet-app/src/ipc.rs` | 新增 `get_config` / `save_config` |
| 3 | `crates/deskpet-app/frontend/settings.html` | 新建：设置页面（纯 HTML + 原生 JS） |
| — | `tauri.conf.json` / Cargo 依赖 | **零改动** |

---

## 3. 详细设计

### 3.1 托盘入口

- 在「立即播报」与「退出」之间插入：

```rust
let settings_item = MenuItemBuilder::with_id("settings", "设置").build(app)?;
let menu = MenuBuilder::new(app)
    .items(&[&show_item, &hide_item, &agent_item, &settings_item, &quit_item])
    .build()?;
```

- `on_menu_event` 增加分支：

```rust
"settings" => { let _ = ensure_settings_window(app); }
```

### 3.2 设置窗口

仿照 `ensure_bubble_window` 的懒建复用模式：

```rust
fn ensure_settings_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(w);
    }
    WebviewWindowBuilder::new(app, "settings", tauri::WebviewUrl::App("settings.html".into()))
        .title("DeskPet 设置")
        .inner_size(560.0, 680.0)
        .min_inner_size(480.0, 560.0)
        .resizable(true)
        .center()
        .build()
}
```

- 窗口属性：普通带边框窗口，不置顶、不 `skip_taskbar`、可缩放。
- 关闭行为：监听 `CloseRequested` → `api.prevent_close()` + `hide()`，实现「关闭即隐藏、再打开复用」，避免反复创建 webview。
- 每次 `show()` 触发前端重新拉取最新配置（`focus` 或 `visibilitychange` 事件中重载）。

### 3.3 新增 IPC

#### `get_config`

- 返回结构体 `ConfigDto`（字段与 `AppConfig` 对齐，`serde` 直接沿用）。
- **`api_key` 脱敏**：
  - 空 → `""`
  - 长度 ≤ 8 → `"****"`
  - 其他 → `"前4...后4"`
- 额外附带 `has_api_key: bool`，便于前端提示「已配置」。

#### `save_config`

处理流程：

1. 后端校验（见 3.5）。
2. **API Key 合并规则**：若传入 `api_key` 为空字符串，则沿用磁盘上已有的 key；只有当用户显式输入新值才覆盖。避免脱敏串或空值把真 key 冲掉。
3. `AppConfig::save()` 写回 `config.toml`。
4. **立即 `state.reload_config().await`**，不依赖 watcher 的 5 秒轮询，保存即生效。
5. 返回 `Ok(true)`；失败返回可读错误文案。

> 画像保存复用现有 `update_profile`，不新增命令。

### 3.4 设置页分区（settings.html）

| 分区 | 控件 | 绑定字段 |
| --- | --- | --- |
| 常规 | 勿扰时段列表（增 / 删 / 改，`time` 输入对）、最小播报间隔（number，分钟）、播报阈值（range 0~1 + 数值回显） | `general.*` |
| LLM | api_base、api_key（password，占位为脱敏值，留空=保持不变）、model、embedding_model（带「留空即关闭 embedding」提示） | `llm.*` |
| 采集平台 | rsshub_endpoint + 六个平台各自的启用开关、间隔分钟 | `fetch.*` |
| 语音 | 启用开关、voice 下拉；标注「需重启应用生效」 | `tts.*` |
| 用户画像 | persona 文本域、interests 列表（topic + weight 0~1，可增删） | `update_profile` |

页面交互：

- 打开时 `Promise.all([invoke('get_config'), invoke('get_profile')])` 填充。
- 底部「保存」：先本地校验 → `save_config` → `update_profile` → 顶部 toast「已保存并生效」。
- 保存失败时展示后端错误原文，不清空用户输入。

### 3.5 校验规则（前后端各一份）

- `quiet_hours`：每条匹配 `^\d{2}:\d{2}-\d{2}:\d{2}$`，且时 0~23、分 0~59。
- `min_announce_interval_min >= 1`。
- 各平台 `interval_min >= 1`。
- `0 <= announce_threshold <= 1`。
- `api_base` 必须以 `http://` 或 `https://` 开头。
- `api_key` 非空时不做格式强校验（各厂商 key 格式不一）。
- 画像 `weight` 裁剪到 `0.0..=1.0`。

### 3.6 IPC 注册

`gui.rs` 的 `invoke_handler!` 追加：

```rust
ipc::get_config,
ipc::save_config,
```

---

## 4. 已知限制与风险

| 项 | 说明 |
| --- | --- |
| TTS 不热更 | `reload_config` 明确不重建 TTS 实例；页面上标注「重启生效」。 |
| Agent 循环间隔 | `gui.rs` 后台 loop 的 `sleep` 取的是启动时快照，热更后需重启才完全对齐（scheduler 内部间隔是热更的，影响很小）。 |
| API Key 安全 | 读取全程脱敏；写入时空值=保留原值，杜绝脱敏串覆盖真 key。config.toml 为明文存储，与现有行为一致。 |
| 窗口生命周期 | 设置窗口隐藏而非销毁，webview 常驻有少量内存占用，换取秒开。 |
| 多显示器 | `center()` 以主屏为准；如需跟随宠物所在屏，可后续改为按 `pet` 窗口位置偏移。 |

---

## 5. 验证步骤

本机（当前环境）缺少 C 编译/链接器，无法本地编译，请在目标环境执行：

1. 编译检查：
   ```bash
   cargo check -p deskpet-app --features tauri
   ```
2. 启动应用，右键托盘图标 → 观察菜单出现「设置」。
3. 点击「设置」→ 窗口打开；再次点击 → 窗口前置聚焦（不重复创建）。
4. 修改勿扰时段并保存 → 日志出现：
   ```
   [reload] policy updated: quiet_hours=[...], min_interval=...min
   ```
5. 修改 LLM `api_base/model/key` 并保存 → 日志出现 `[reload] LLM updated: ...`，key 为脱敏显示。
6. 仅修改 persona / interests 保存 → 重启后仍存在（落库验证）。
7. 关闭设置窗口 → 再次打开，字段值为最新配置（确认未读缓存）。
8. 关闭再打开应用 → 配置持久化正常（config.toml 内容正确、无字段丢失）。

---

## 6. 实施顺序

1. `ipc.rs`：新增 `get_config` / `save_config`（含脱敏与 key 合并）。
2. `gui.rs`：菜单项、事件分支、`ensure_settings_window`、关闭拦截、IPC 注册。
3. `frontend/settings.html`：页面结构与交互。
4. 前端样式对齐现有 `index.html` / `bubble.html` 的视觉风格。
5. 本地静态检查（fmt / clippy 视环境而定）→ 交由用户环境编译验证。
