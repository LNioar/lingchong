# DeskPet Android 移植可行性报告

> 日期：2026-10-20
> 依据：`implementation-status.md`（2026-10-19 源码静态审查）、[android-porting-plan.md](file:///d:/Users/admin/Desktop/deskpet-src/docs/android-porting-plan.md)、Cargo 工作区实际依赖。
> 结论先行：**可行，且架构条件比一般桌面应用好**。核心业务代码约 70% 可直接复用；真正的重写集中在 GUI 层（双窗口 + 托盘）、TTS、后台保活三处。

---

## 1. 结论

| 维度 | 评估 |
|---|---|
| 技术可行性 | ✅ 可行。Tauri 2 官方支持 Android 目标；核心依赖（reqwest/rustls、rusqlite/bundled、tokio）均有 Android 编译路径 |
| 代码复用度 | 🟢 高。5 个平台无关 crate（core / storage / collector / scheduler / agent）基本零改动 |
| 主要重写点 | 🔴 3 处：`gui.rs` 双窗口与托盘（桌面概念 Android 不存在）、`deskpet-tts`（edge-tts CLI 不可用）、后台保活（Doze/厂商杀进程） |
| 最大不确定性 | ⚠️ **后台定时采集模型**。Android 的 Doze 与国内厂商后台限制，意味着桌面版「tokio 定时循环」的架构不能直接搬，必须引入 WorkManager/前台服务桥接 |
| 建议 | 采用「Tauri 2 Android + 复用核心 crate」方案，分 7 阶段推进（见第 6 节） |

---

## 2. 各 Crate 复用性评估（基于实际依赖）

| Crate | 关键依赖 | Android 兼容性 | 复用度 | 需要做的事 |
|---|---|---|---|---|
| `deskpet-core` | serde / chrono / sha2 | ✅ 纯类型 | ~100% | 无 |
| `deskpet-storage` | rusqlite `bundled`（SQLite 源码编译） | ✅ NDK 可编译，需设置 `CC_aarch64_linux_android` 等环境变量 | ~95% | 交叉编译验证；数据目录注入 |
| `deskpet-collector` | reqwest `rustls-tls`（不依赖系统 TLS） | ✅ | ~95% | 验证平台可达性（RSSHub 实例、微博/B站接口在国内网络下与桌面一致） |
| `deskpet-agent` | HTTP LLM 调用 | ✅ | ~95% | 无结构性改动 |
| `deskpet-scheduler` | tokio 定时循环 | 🟡 代码可编译，**但 Android 后台下定时器不可靠** | ~70% | 采集触发改由 WorkManager/AlarmManager 唤醒，tokio 循环退化为「前台时运行」 |
| `deskpet-tts` | 调用 `edge-tts` CLI 子进程 | ❌ Android 无 Python/CLI | ~10% | 重写：HTTP 直连微软 TTS，或 JNI 桥接 Android 原生 TextToSpeech |
| `deskpet-app`（GUI 部分） | tauri `tray-icon`、`WebviewWindow`、`cmd /C start` | ❌ 双窗口/托盘/悬浮窗是桌面概念 | ~20% | 整体重写为移动端 UI（见第 4 节） |
| `deskpet-app`（非 GUI 部分） | config / logging / runtime / ipc | 🟡 逻辑可复用，路径与入口需改 | ~70% | 抽离 `dirs::data_dir()`（Android 返回 `None`），注入 app 私有目录 |
| 前端 `index.html` / `bubble.html` | SVG + CSS + Tauri IPC | 🟡 WebView 兼容，但交互范式不匹配 | ~40% | 触摸事件替换鼠标/滚轮；竖屏卡片式布局重写 |

### 代码中已确认的桌面特定调用点（必须改造清单）

| 位置 | 问题 | Android 方案 |
|---|---|---|
| [config.rs](file:///d:/Users/admin/Desktop/deskpet-src/crates/deskpet-app/src/config.rs) `dirs::data_dir()` | Android 返回 `None` | 抽象 `Paths` 结构，Android 下用 Tauri `app_data_dir()` |
| [runtime.rs](file:///d:/Users/admin/Desktop/deskpet-src/crates/deskpet-app/src/runtime.rs) `dirs::data_dir()` | 同上 | 同上 |
| `logging.rs` 日志文件路径 | 同上 | 同上 |
| [ipc.rs](file:///d:/Users/admin/Desktop/deskpet-src/crates/deskpet-app/src/ipc.rs) `open_url` 用 `cmd /C start` / `xdg-open` | 子进程调用不存在 | 改用 Android Intent（`android.intent.action.VIEW`），顺带消除 B6 命令注入面 |
| [gui.rs](file:///d:/Users/admin/Desktop/deskpet-src/crates/deskpet-app/src/gui.rs) 整个文件 | 透明无边框窗口、气泡定位、托盘菜单 | 全部替换（宠物悬浮窗走 `SYSTEM_ALERT_WINDOW` overlay，或放弃悬浮改为 App 内 UI + 通知） |
| `deskpet-tts/src/edge_tts.rs` | `std::process::Command` 调 CLI | 重写为 HTTP/WebSocket 合成 |

---

## 3. 三条技术路线对比

| 方案 | 说明 | 优点 | 缺点 | 复用度 |
|---|---|---|---|---|
| **A. Tauri 2 Android（推荐）** | `cargo tauri android init`，核心 crate 原样链接，UI 仍用 WebView | 复用最大化；一套前端技术栈；官方支持 | 后台能力弱，需 Kotlin 写前台服务桥接；WebView 版本碎片 | ~70% |
| B. 原生 Kotlin + Rust core（JNI/uniffi） | UI 全部 Jetpack Compose 重写，Rust 只保留核心库 | 性能与系统集成都最好；后台/通知/Widget 原生体验 | 前端与 gui/ipc 全量重写，工作量约方案 A 的 2 倍 | ~55% |
| C. 降级形态：App + 通知，无悬浮宠物 | 不做 overlay 悬浮窗，播报走系统通知 + App 内展示 | 规避悬浮窗权限（国内厂商限制多、用户授权率低）；实现最简 | 失去「桌面宠物」的核心卖点 | ~75% |

**建议组合**：以方案 A 为主线，首版采用方案 C 的播报形态（通知 + App 内气泡页），悬浮宠物窗作为后续可选增强（需引导用户授权 `SYSTEM_ALERT_WINDOW`）。

---

## 4. 关键风险与缓解

| 风险 | 等级 | 说明 | 缓解 |
|---|---|---|---|
| 后台定时采集不可靠 | 🔴 高 | Doze 模式与国内厂商（小米/华为/OPPO）杀后台会让 tokio 定时循环静默失效，这是桌面架构在 Android 上的**本质性失配** | 前台服务（常驻通知）保调度 + WorkManager 定时唤醒采集（最短间隔 15 分钟）；采集间隔默认值从桌面节奏放宽到 30 分钟 |
| rusqlite bundled NDK 编译失败 | 🟡 中 | 历史上 `bundled` 在 Android 目标可编译，但需正确配置 NDK toolchain 环境变量 | 阶段 2 第一件事做编译验证（`cargo build -p deskpet-storage --target aarch64-linux-android`），失败则换 `sqlx` |
| 悬浮窗权限收紧 | 🟡 中 | `SYSTEM_ALERT_WINDOW` 需用户手动到系统设置授权，国内 ROM 还有额外限制 | 首版不依赖悬浮窗（通知形态）；悬浮窗作为可选功能，提供权限引导页 |
| Edge TTS 网络可达性 | 🟡 中 | HTTP 直连微软 TTS 在国内可能不稳定；且桌面版 P1 已记录「Windows 无播放器」问题 | 实现 HTTP TTS + Android 原生 TTS 双后端，配置切换；原生 TTS 离线兜底 |
| WebView 版本碎片 | 🟡 中 | 前端 SVG 动画/CSS 在旧 WebView 可能异常 | 最低支持 Android 8.0（WebView 可独立升级）；真机覆盖 3 品牌验证 |
| 内存/电量 | 🟢 低 | embedding 全量重算 + 暴力余弦检索在手机上有电量压力 | 顺带实施桌面版 P2 已有的「embedding 24h 标题缓存」计划；限制候选集大小 |
| LLM 不可用降级 | 🟢 低 | 已有完善降级链（重试→模板文案、哈希向量占位），直接继承 | 无需额外工作 |

---

## 5. 前置依赖（建议移植前先完成）

移植会把桌面版现有缺陷带进 Android，以下项目建议先修（均来自 implementation-status.md 的 P0/P1）：

1. **B1**（勿扰/无内容仍弹气泡）——在 Android 上这个缺陷会变成「勿扰时段弹系统通知」，体验更糟，**必须先修**。
2. **反馈学习闭环**（多数平台无 tags）——移动端同样依赖画像质量，且移动端重构 UI 时正好补 `click`/`ignore` 反馈类型。
3. **B4**（配置解析失败→内存全默认）——移动端用户无法手改 config.toml，配置容错更关键。
4. **语义去重落地**（`SameEventAs` 未合并）——移动端通知打扰成本远高于桌面气泡，重复播报不可接受。
5. Windows GUI 安装包尚未产出（P0）——建议桌面版先在 Windows 实机验证双窗口，再把已验证的交互语义移植，避免在两个平台同时调 UI 行为。

> 注意：当前环境无 `cargo`，本报告与 implementation-status 一样，**不声称编译通过**；上述 rusqlite/tauri 的 Android 编译性需在阶段 1–2 实测。

---

## 6. 实施路线（7 阶段，与既有移植计划对齐）

| 阶段 | 目标 | 关键产出 | 验收标准 |
|---|---|---|---|
| 1. 环境搭建 | Android 交叉编译环境 | NDK r25c+、`aarch64-linux-android` target、`cargo tauri android init` | 空白 Tauri Android 项目在模拟器运行 |
| 2. 核心 crate 适配 | 5 个非 GUI crate 编译通过 | `Paths` 路径抽象；TTS HTTP 后端；rusqlite 交叉验证 | `cargo build -p deskpet-storage --target aarch64-linux-android` 通过 |
| 3. Tauri Android 集成 | deskpet-app 在 Android 跑起来 | `gen/android/`、AndroidManifest 权限、前台服务 Kotlin 骨架 | 模拟器内采集/调度日志正常输出 |
| 4. 前端移动化 | 竖屏触摸 UI | 卡片式布局、touch 事件替换 mouse/wheel、通知式播报 | 模拟器 UI 渲染正常、后台通知可弹出 |
| 5. 后台保活 | 切后台后采集继续 | 前台服务 + WorkManager 定时唤醒 + 网络状态检查 | 切后台 10 分钟后仍有采集日志 |
| 6. 功能完善 | 应用内设置（消灭手改 config.toml） | 设置页（API Key/平台开关/勿扰/语音）、权限引导、分享 | 全部设置可在 App 内完成 |
| 7. 打包发布 | 可分发产物 | 签名 APK/AAB、arm64+arm32+x86_64 | 3 台不同品牌真机（Android 8+）正常运行 |

工作量估算：修改约 11 个文件、新增约 17 个文件（Kotlin 服务、AndroidManifest、设置页、移动端 CSS 等），其中阶段 3 和 5 复杂度最高。

---

## 7. 总体判断

- **值得做**：架构分层干净（core/storage/collector/scheduler/agent 单向依赖、平台无关），是这次移植最大的资产；前端也是 Web 技术栈，迁移成本低。
- **不要照搬的地方**：桌面版「常驻进程 + tokio 定时循环」的运行模型在 Android 上行不通，采集触发必须让位给 WorkManager；「悬浮宠物」应降级为可选增强而非核心路径。
- **正确顺序**：先修 P0 缺陷（尤其 B1 与反馈学习闭环）→ 桌面 Windows 版实机验证 → 再启动 Android 移植阶段 1。把带缺陷的链路移植过去，会在移动端放大问题。
