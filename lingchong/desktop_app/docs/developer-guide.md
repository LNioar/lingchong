# 开发者指南

更新日期：2026-10-24。默认工作目录为 `desktop_app/`，完整布局见 [项目结构](project-structure.md)。同级备份目录不参与构建。

## 环境与源码入口

使用当前锁定依赖支持的 Rust 工具链。workspace 虽声明 `rust-version = "1.75"`，但不代表所有锁定的 Tauri 依赖能在该版本编译；以实际依赖要求和构建结果为准。

Windows GUI 需要对应 Rust 工具链的编译工具及 WebView2；Linux GUI 需要 WebKitGTK、GTK3、libsoup 和托盘依赖。Android 还需要 Tauri CLI 2、JDK、SDK、NDK 与对应 Rust target。具体 Android 本机配置和排障见 [Android 工程指南](android-studio.md)。前端为静态 HTML/CSS/JavaScript，没有 npm 构建步骤；前端回归脚本使用 Node.js，数据库分析脚本使用 Python。

`src/main.rs` 调用 `deskpet_app_lib::run()`；`src/lib.rs` 提供共享入口，并按 `tauri` feature 选择 GUI 或 headless。Android 通过同一个库的移动端入口启动。

## 从 workspace 根目录运行

```powershell
# 无 GUI 的后台模式
cargo run -p deskpet-app

# 桌面宠物 GUI
cargo run -p deskpet-app --features tauri

# 编译检查
cargo check -p deskpet-app --features tauri

# workspace 构建
cargo build --workspace

# GUI 发布二进制，不等同于安装包
cargo build -p deskpet-app --features tauri --release
```

## 测试与文档检查

```powershell
cargo test --workspace
cargo test -p deskpet-agent
cargo test -p deskpet-app --lib

node scripts/test-pet-ui.cjs
node scripts/test-bubble-ui.cjs
node --check crates/deskpet-app/frontend/mobile.js
node --check crates/deskpet-app/frontend/settings.js
node scripts/check-docs.cjs
```

仅在依赖缓存完整时添加 `--offline`。现有前端测试用最小 DOM 与模拟时钟验证交互，不代替 WebView 渲染、Android 生命周期或设备触摸验收。不要通过固定测试总数判断版本是否完整。

## Windows 打包

以下命令先进入应用 crate；该目录包含 `tauri.conf.json`。

```powershell
Set-Location crates/deskpet-app
cargo tauri build --features tauri --bundles msi
```

默认 MSI 输出到 workspace 的 `target/release/bundle/msi/`，以构建日志为准。首次使用需准备 Tauri CLI，打包工具可能另外下载。详细依赖与排障见 [Windows 指南](windows-guide.md)。

## Android 构建与入口

从 workspace 根目录开始，进入应用 crate：

```powershell
Set-Location crates/deskpet-app

# 调试 / 启动 Android Studio，保持 CLI 进程运行
cargo tauri android dev --features tauri --open

# 生成 Android APK
cargo tauri android build --features tauri --apk
```

已有工程为 `crates/deskpet-app/gen/android/`，请勿重新初始化覆盖项目定制。Gradle APK 通常在该工程的 `app/build/outputs/apk/` 下；按目标架构和构建变体查找，以本次成功构建日志为准。前端静态资源来自 `frontend/`，Android 启用 `custom-protocol` 使用包内资源；实际联网采集仍需网络。

当前 Activity 由 Manifest 声明横屏，MainActivity 负责沉浸式系统栏与前台亮屏。界面主要修改 `mobile.css` / `mobile.js`，不应只修改 APK 或 Gradle 中间目录。

## 调试与数据

- 配置、日志、数据库通过 `src/paths.rs` 定位；Windows 通常为 `%APPDATA%\deskpet\`，Android 由启动入口注入应用私有目录。
- `scripts/diagnose-announce.ps1`、`scripts/analyze-database.py` 可辅助排查；运行前查看脚本参数和本机路径。
- `cargo run -p deskpet-dump` 只导出知乎和 B站各 Top10 热点，不是完整数据库备份或反馈/日志导出。
- 日志检索可关注 `[scheduler]`、`[agent]`、`[watcher]`、`[feedback]`。日志可能含请求和画像内容。
- `.tools/`、`diagnostics/` 为本地工具与快照，不属于可复现构建的必需依赖。

## 修改功能时同步维护

新增采集平台需同时检查 collector 的适配器和 manager、app 默认配置、runtime 注册、设置验证与平台选项；新增配置需检查 DTO、持久化、旧配置兼容与热更新范围；新增情绪需检查 core 枚举、前端资源配置和业务触发。

交付前分别核对源码测试、安装包生成、设备安装和实际行为，不将其中一步通过等同于全部完成。功能与限制同步到 [产品说明书](user-manual.md)、[实现状态](../implementation-status.md)；规划维护在 [路线图](roadmap.md)。
