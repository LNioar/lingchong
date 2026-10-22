# DeskPet Android 平台移植文档

> 版本：2026-10-20（依据当日源码状态修订，取代旧版计划）
> 前置阅读：[android-feasibility-report.md](file:///d:/Users/admin/Desktop/deskpet-src/docs/android-feasibility-report.md)（可行性论证与路线对比）

---

## 0. 当前进度

| 事项 | 状态 |
|---|---|
| 移植前置修复 B1（勿扰/无内容弹气泡） | ✅ 已完成（gui.rs 定时循环仅在 `Some` 时播报） |
| 反馈学习闭环（tags 来源 + click/ignore/dismiss 补全） | ✅ 已完成（tagging.rs 关键词打标 + bubble.html 全量反馈） |
| 阶段 2a：数据目录抽象 `paths.rs` | ✅ 已完成，桌面行为不变，Android 注入点已预埋（gui.rs setup 开头） |
| 阶段 1：Android 开发环境 | ✅ 已完成（2026-10-20：JAVA_HOME 修正、cmdline-tools 归位 `latest`、NDK 29.0.13846066 装好且 license 已接受、Rust 4 个 android target、`cargo tauri android init` 生成 `crates/deskpet-app/gen/android`） |
| 阶段 2c：核心 crate 交叉编译验证 | ✅ 已通过（6 个 crate `--target aarch64-linux-android` check 全绿，rusqlite bundled 经 NDK clang 编译成功，约 60s） |
| 阶段 2b：TTS HTTP 直连重写 | ⬜ 未开始 |
| 阶段 3~7 | ⬜ 未开始（下一步：创建 AVD 后 `cargo tauri android dev`） |

> 环境备注：`JAVA_HOME`/`ANDROID_HOME`/`NDK_HOME` 已写入用户环境变量，**需重开终端生效**；本机 cmdline-tools 已从 `cmdline-tools\bin` 归位到规范的 `cmdline-tools\latest\bin`。JDK 27 实测可驱动 sdkmanager 19.0。交叉编译需设置 `CC_aarch64_linux_android`/`AR_aarch64_linux_android`（见阶段 2c 命令）。

技术路线（已定）：**Tauri 2 Android + 复用 5 个核心 crate**；首版播报形态为「系统通知 + App 内 UI」，悬浮宠物窗作为可选增强。

---

## 1. 架构复用总览

```
deskpet-app（GUI 层重写：双窗口+托盘 → 移动端 UI + 前台服务）
   ├── deskpet-agent       ✅ 直接复用（HTTP LLM）
   ├── deskpet-scheduler   🟡 代码复用，触发源改 WorkManager/前台服务
   ├── deskpet-collector   ✅ 直接复用（reqwest/rustls；已含 tagging 打标）
   ├── deskpet-tts         ❌ 重写（edge-tts CLI → HTTP 直连/原生 TTS）
   └── deskpet-storage     ✅ 直接复用（rusqlite bundled，需 NDK 编译验证）
          └── deskpet-core ✅ 直接复用（纯类型）
```

已确认的桌面特定调用点（必须改造）：

| 位置 | 问题 | Android 方案 | 状态 |
|---|---|---|---|
| config.rs / runtime.rs / logging.rs 的 `dirs::data_dir()` | Android 返回 None | 已统一走 [paths.rs](file:///d:/Users/admin/Desktop/deskpet-src/crates/deskpet-app/src/paths.rs)，启动时注入 app 私有目录 | ✅ |
| [ipc.rs](file:///d:/Users/admin/Desktop/deskpet-src/crates/deskpet-app/src/ipc.rs) `open_url` 用 `cmd /C start` / `xdg-open` | 子进程不存在 | Android Intent `ACTION_VIEW`（Tauri opener 插件或 JNI） | ⬜ |
| [gui.rs](file:///d:/Users/admin/Desktop/deskpet-src/crates/deskpet-app/src/gui.rs) 双窗口/托盘/气泡定位 | 桌面窗口概念 | 重写：单 Activity + WebView；播报走通知 + 应用内气泡页 | ⬜ |
| [edge_tts.rs](file:///d:/Users/admin/Desktop/deskpet-src/crates/deskpet-tts/src/edge_tts.rs) 调 `edge-tts` CLI | 无 Python | HTTP/WebSocket 直连微软 TTS；备选 Android 原生 TTS | ⬜ |
| 前端 index.html / bubble.html | 鼠标/滚轮交互、桌面小窗范式 | 触摸事件 + 竖屏卡片式布局 | ⬜ |

---

## 2. 阶段实施

### 阶段 1：环境搭建

1. 安装 Android Studio（含 SDK Platform 24+、SDK Build-Tools）
2. 安装 NDK r25c+（SDK Manager → SDK Tools → NDK Side by side）与 JDK 17
3. 配置环境变量：`ANDROID_HOME`、`NDK_HOME`（或 `ANDROID_NDK_HOME`）
4. Rust 目标与 Tauri 初始化：

```powershell
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo install tauri-cli --version "^2.0"
cd crates/deskpet-app
cargo tauri android init
```

**验收**：`cargo tauri android dev` 在模拟器显示默认页面。

### 阶段 2：核心 crate 适配

**2a. 数据目录抽象 — 已完成** ✅

[paths.rs](file:///d:/Users/admin/Desktop/deskpet-src/crates/deskpet-app/src/paths.rs) 提供 `init(dir)` / `data_dir()`；[gui.rs](file:///d:/Users/admin/Desktop/deskpet-src/crates/deskpet-app/src/gui.rs#L148-L165) 在 setup 最先执行 `#[cfg(target_os = "android")]` 注入 `app.path().app_data_dir()`，随后才初始化 logging/config/db。**注意保持此顺序**：任何在注入前读取路径的代码都会落到错误目录。

**2b. TTS 重写**

新增 HTTP 后端替代 CLI 调用：

```rust
// deskpet-tts/src/http_tts.rs（新建）
pub struct HttpEdgeTts { voice: String, client: reqwest::Client }

#[async_trait]
impl TtsProvider for HttpEdgeTts {
    async fn speak(&self, text: &str) -> Result<()> {
        // POST 微软 TTS 接口合成音频字节流，再交给平台播放
    }
}
```

播放端：桌面沿用现有播放器逻辑；Android 通过 JNI 调 `MediaPlayer` 或直接走 `TextToSpeech` API 合成+播放一体。

`runtime.rs` 中按 `cfg(target_os)` 选择实现：

```rust
#[cfg(target_os = "android")]
let tts: Option<Box<dyn TtsProvider>> = ... // HttpEdgeTts 或 AndroidTts
#[cfg(not(target_os = "android"))]
let tts: Option<Box<dyn TtsProvider>> = ... // 现有 EdgeTts
```

**2c. rusqlite 交叉编译验证（第一优先，阻塞项）**

```powershell
$env:CC_aarch64_linux_android = "$env:NDK_HOME\toolchains\llvm\prebuilt\windows-x86_64\bin\aarch64-linux-android24-clang.cmd"
$env:AR_aarch64_linux_android = "$env:NDK_HOME\toolchains\llvm\prebuilt\windows-x86_64\bin\llvm-ar.exe"
cargo build -p deskpet-storage --target aarch64-linux-android
```

失败则评估 `sqlx` + `libsqlite3-sys` 替代。

**验收**：5 个核心 crate 全部 `--target aarch64-linux-android` 编译通过。

### 阶段 3：Tauri Android 集成

**3a. AndroidManifest.xml 权限**

```xml
<uses-permission android:name="android.permission.INTERNET" />
<uses-permission android:name="android.permission.ACCESS_NETWORK_STATE" />
<uses-permission android:name="android.permission.FOREGROUND_SERVICE" />
<uses-permission android:name="android.permission.POST_NOTIFICATIONS" />
<!-- 可选（悬浮宠物增强）： -->
<uses-permission android:name="android.permission.SYSTEM_ALERT_WINDOW" />
```

**3b. 前台服务 `DeskPetService.kt`（新建）**

- `onStartCommand`：`startForeground(id, notification)` 常驻通知「DeskPet 正在运行」，随后拉起 Rust 调度
- `START_STICKY` 保活；`onDestroy` 清理资源

**3c. open_url 改造**

用 tauri-plugin-opener（官方插件，内部走 Android Intent）替换 `cmd /C start` / `open` / `xdg-open` 三分支，顺带消除 B6 命令注入面。桌面行为不变。

**3d. tauri.conf.json**

```json
{
  "app": {
    "security": { "csp": "default-src 'self'; connect-src 'self' https: wss:" }
  }
}
```

（移动端建议顺带补上 CSP，桌面版当前 `csp: null`。）

**验收**：模拟器启动后日志显示采集/调度运行，数据库创建于 app 私有目录。

### 阶段 4：前端移动端适配

| 桌面 | 移动 |
|---|---|
| 透明无边框 190×222 宠物窗 | 全屏 App，宠物居中展示 |
| 独立 300×180 气泡窗 | 底部 Sheet / 卡片弹出 |
| 鼠标拖拽（位移 >3px） | 触摸拖动 |
| 双击立即播报 | 双击（touchstart 计时）保留 |
| 滚轮切情绪 | 左右滑动手势 |
| 托盘菜单 | App 内设置入口 + 前台服务常驻通知 |

播报通知化（后台时）：
- 标题 = 热点标题，正文 = 口播文案
- 点击通知 → 打开 App 展示气泡详情（复用 bubble 页逻辑）
- 通知内 action 按钮「喜欢/不感兴趣」→ 直连 `feedback` IPC（feedback 已支持全量 action）

宠物 SVG 动画与精灵图引擎在 Android WebView 兼容，无需改动。

**验收**：竖屏 UI 渲染正常，触摸交互可用，锁屏/后台时通知弹出且 action 生效。

### 阶段 5：后台保活

**5a. 调度模型改造（核心差异）**

桌面「常驻进程 + tokio 定时循环」在 Android 不可行（Doze + 厂商杀后台）。改为双轨：

| 状态 | 采集触发 | 播报触发 |
|---|---|---|
| App 前台 | 现有 tokio 循环 | 现有 tokio 循环 |
| App 后台/灭屏 | `WorkManager` 周期任务（≥15 分钟）唤醒 → JNI 调 `fetch_platform` | 前台服务内评估 `can_announce` → 命中则发系统通知 |

**5b. 省电与网络**

- `WorkManager` 约束：`setRequiredNetworkType(CONNECTED)`，无网跳过
- 采集默认间隔放宽至 30 分钟（配置可热更新机制保留）
- 勿扰/静默/最小播报间隔策略全部沿用 deskpet-scheduler 现有 policy（B1 已修，勿扰时段不会再弹通知）

**验收**：切后台 10 分钟后仍有采集日志；灭屏 30 分钟 WorkManager 正常唤醒；勿扰时段无通知。

### 阶段 6：移动端功能完善

**6a. 应用内设置页（必须，移动端无法手改 config.toml）**

复用已有 IPC（`reload_config` / `silence_for` / `get_profile` / `update_profile`），新增配置读写命令。页面项：
- LLM API Key（密码框）/ api_base / model
- 平台开关、采集间隔滑块
- 勿扰时段选择器、语音开关、TTS 后端切换（HTTP/原生）

**6b. 权限引导**：Android 13+ 通知权限首启申请；悬浮窗为可选增强，单独引导页。

**6c. 分享**：口播内容走 Android Share Sheet。

**6d. Widget（可选）**：桌面小部件显示宠物状态 + 最新播报。

### 阶段 7：打包发布

```powershell
cargo tauri android build        # APK
cargo tauri android build --aab  # AAB（Google Play）
```

- 配置 release 签名（keystore 入 `gen/android/app/build.gradle.kts`）
- 多架构：arm64（主）+ armv7 + x86_64（模拟器）
- 真机验证清单：3 品牌（小米/华为/OPPO 优先，杀后台策略最激进）、Android 8.0+、后台保活、电量消耗、断网恢复、通知 action

---

## 3. 风险与缓解

| 风险 | 等级 | 缓解 |
|---|---|---|
| 后台被杀导致采集/播报中断 | 🔴 高 | 前台服务 + WorkManager 双轨；真机重点测国内 ROM；勿扰策略已修（B1 ✅） |
| rusqlite bundled NDK 编译失败 | 🟡 中 | 阶段 2c 第一优先验证；备选 sqlx |
| Edge TTS 网络不可达 | 🟡 中 | HTTP/原生双后端可切换 |
| 悬浮窗权限授权率低 | 🟡 中 | 首版不依赖悬浮窗（通知形态），悬浮宠物列为可选增强 |
| WebView 版本碎片 | 🟡 中 | 最低 Android 8.0（WebView 可独立升级） |
| embedding 全量重算的电量/流量成本 | 🟢 低 | 后续实施 24h 标题缓存（桌面 P2 项，双端共享收益） |

---

## 4. 工作量估算

| 阶段 | 修改 | 新增 | 复杂度 | 状态 |
|---|---|---|---|---|
| 前置修复（B1 + 反馈闭环） | 3 | 1 | 中 | ✅ 完成 |
| 1. 环境搭建 | 0 | 3 | 低 | ⬜ |
| 2. 核心 crate 适配 | 4 | 2（paths ✅ + http_tts） | 中 | 🟡 2a 完成 |
| 3. Tauri Android 集成 | 3 | 5（Kotlin/manifest） | 高 | ⬜ |
| 4. 前端适配 | 2 | 1（移动端 CSS/布局） | 中 | ⬜ |
| 5. 后台保活 | 1 | 3（WorkManager/Service） | 高 | ⬜ |
| 6. 功能完善 | 1 | 2（设置页/分享） | 中 | ⬜ |
| 7. 打包发布 | 2 | 2（签名/CI） | 低 | ⬜ |

---

## 5. 验证基线

- 桌面回归基线：全工作区 53 个单元测试通过（2026-10-20，cargo 1.99.0），tauri/headless 两种 feature 均编译通过。移植期间每阶段完成后须保持该基线不劣化。
- Android 新增验证：`--target aarch64-linux-android` 编译矩阵、模拟器冒烟、3 品牌真机场景清单（阶段 7）。
