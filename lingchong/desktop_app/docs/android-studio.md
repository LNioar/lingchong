# Android Studio 加载排查

## 已确认的问题（2026-10-21）

- 当前 Android Studio 为 Panda 4 / 2025.3.4 Patch 1，IDE 日志声明支持的最高 AGP 为 9.2.1；原工程使用 AGP 9.3.1、Gradle 9.6.1。
- 已将根工程及 buildSrc 的 AGP 统一为 9.2.1，Wrapper 调整为官方对应的 Gradle 9.4.1。保留原有下载镜像及 Kotlin 2.2.10、JDK 21 配置。
- `settings.gradle` 引用的 `tauri.settings.gradle` 和 app 引用的 `tauri.build.gradle.kts` 均缺失。这些是 Tauri 在本机生成的接入文件，不能通过删除 apply 语句或创建空文件解决。
- 按当前机器环境，app 使用 `compileSdkVersion("android-36.1")` / `targetSdk = 36` / `minSdk = 24`。本机只有 android-34、android-35、android-36.1；整数 `compileSdk = 36` 不能准确指定已安装的 android-36.1。本工程启用了旧版 Android DSL，因此使用其字符串重载，无需安装 API 37。
- Tauri CLI 现已安装，但 `android dev` 在自动下载 Android Command-line Tools 时超时；本机 SDK 中仍缺少 cmdline-tools 和 NDK。这一步失败后，两个 Tauri 接入文件尚未生成。
- `settings.gradle` 已改为先完成 settings 加载，再在 projectsLoaded 阶段报告缺少的接入文件，并提前输出同一错误。这样避免主动在 settings 求值阶段抛异常；仍需实际 IDE 同步验证是否消除了次生的 settings 异常。

`The settings are not yet available for build ':'` 出现在 IDE 注入的初始化脚本和 Gradle 模型读取调用栈中；仅凭该调用栈不能确定唯一原因。上面的版本不匹配和生成文件缺失均需处理，随后应重新同步读取首个实际错误。

## 启动方法

在 PowerShell 中，从项目的 `desktop_app` 目录执行：

```powershell
# 先确认 CLI；若提示 no such command，再执行安装命令。
cargo tauri --version
cargo install tauri-cli --version '^2' --locked

Set-Location crates/deskpet-app
cargo tauri android dev --features tauri --open
```

使用 Android Studio 期间保持这个 CLI 进程运行。通过此入口让 Tauri 配置本机依赖及 Rust 构建参数，再在 IDE 中同步 `gen/android` 工程。不要用 `android init` 覆盖现有工程来排查同步问题，否则应先检查它是否覆盖这里固定的工具版本。

Android Studio 的 Gradle 设置使用项目 Wrapper，Gradle JDK 使用 JDK 21（本机 Android Studio 的 `jbr`）。沿用已安装的 API 36.1 和 Build Tools。完整 Rust/Android 构建仍需要 NDK；SDK Tools 中补装 Command-line Tools 和 NDK，或将 NDK_HOME 指向其他已安装的有效 NDK。降低 API 版本不能替代 NDK。

## 验证与后续边界

生成接入文件后，可在 `crates/deskpet-app/gen/android` 执行：

```powershell
.\gradlew.bat help --no-configuration-cache --stacktrace
```

排查 IDE 同步问题时，先看命令行是否能完成配置，再看 IDE 的完整同步日志。不要只截取模型读取失败后的最后一个异常。

本次命令行验证在 Gradle 启动阶段遇到 `Unable to establish loopback connection` / `Invalid argument: connect`，尚未进入项目配置，不能据此认定同步或 APK 构建已经通过。

## `no library targets found` 修复

Android 构建使用 `cargo build --lib`，原来只有 `src/main.rs` 的可执行目标会报 `no library targets found in package deskpet-app`。

现已在 Cargo.toml 添加 `deskpet_app_lib` 库目标（staticlib / cdylib / rlib），模块和共享启动入口移到 `src/lib.rs`，并为移动端入口添加 `tauri::mobile_entry_point`。桌面 `main.rs` 调用同一个库入口，保留 GUI / headless feature 的行为。

GUI 中的桌面托盘和独立气泡窗口已限制为桌面平台；移动端先创建单个宠物 WebView，使用应用事件发送播报内容。Android 首先注入应用私有目录，再初始化配置、日志和数据库。移动端气泡 UI、原生 TTS、外链打开和后台保活仍需按移植计划完善，不能据此认定完整移动端功能已经完成。

NDK 现已识别，Android ARM64 库的 `cargo check --lib --features tauri --target aarch64-linux-android` 已通过（2 条未使用代码警告），Windows GUI 可执行入口的 `cargo check --bin deskpet-app --features tauri` 也已通过。接下来从 `crates/deskpet-app` 重新运行 `cargo tauri android dev --features tauri --open`，验证实际库生成、Gradle 同步和设备启动。

## `buildSrc:compileKotlin` 下载 AGP 失败

如果错误包含 `Could not download gradle-9.2.1.jar` 和 `dl.google.com`，失败发生在依赖下载阶段，尚未编译 Kotlin 源码。

根工程的 buildscript / allprojects，以及独立的 buildSrc 工程现已使用阿里云的 `google` / `central` Maven 镜像。buildSrc 不继承根工程仓库，因此必须单独配置；新增的 `buildSrc/settings.gradle.kts` 也将 kotlin-dsl 插件解析配置为阿里云 `gradle-plugin` / `google` / `central` 镜像。

AGP 插件保持 9.2.1；Gradle Wrapper 必须使用对应的 9.4.1，不能因两者都叫 Gradle 就设置为相同版本。Wrapper 使用 9.4.1，并校验官方 SHA-256。

### IDE 仍从 services.gradle.org 下载 `gradle-9.4.1-src.zip`

这是 Kotlin DSL 的 Gradle 源码附件下载，不是 Maven 依赖。Gradle 9.4.1 会从 Wrapper 的镜像目录推导同版本 `-src.zip` 地址；如果镜像没有源码包，会回退到官网。原阿里云 macports 目录的 `gradle-9.4.1-src.zip` 实测返回 404。

因此 Wrapper 已改为 `https://repo.huaweicloud.com/gradle/gradle-9.4.1-bin.zip`，使源码附件同时指向华为云的同目录 `gradle-9.4.1-src.zip`。Maven 仓库仍使用阿里云。Gradle 版本和运行包 SHA-256 均未改变。因为镜像 URL 改变，Wrapper 可能重新缓存一次运行包。

取消正在进行的同步，再重新 Sync Project with Gradle Files，才能让新下载使用更新后的地址。

独立验证 buildSrc（从 `crates/deskpet-app/gen/android` 执行，无需先生成 Tauri 接入文件）：

```powershell
.\gradlew.bat -p buildSrc compileKotlin --no-configuration-cache --console=plain
```

镜像配置用于 Maven 构建依赖，不改变 SDK Manager 的 SDK / NDK 下载源。

## release 优化配置导致项目求值失败

`Cannot use optimization.enable=true without setting android.r8.gradual.support flag` 是原 release 模板使用实验性 R8 配置、但未启用对应开关造成的。app 已改为标准 `isMinifyEnabled = true`，并加载 `proguard-android-optimize.txt` 和现有 Tauri ProGuard 规则，保留 release 的代码压缩优化。

同时出现的 `mergeArm64ReleaseJniLibFolders not found` 是 Android 插件配置失败后的连带错误。修复 R8 配置后，使用现有 Gradle 9.4.1 执行 `help --no-configuration-cache` 已返回 `BUILD SUCCESSFUL`；Rust 插件没有改动。此验证覆盖工程配置和任务注册，尚未验证完整 release APK 构建。

## `tauri-android` 仍请求 `platforms;android-36`

app 的 compileSdk 配置不会自动传给其他 Android 模块。Cargo registry 中的 Tauri Android 库单独声明了 `compileSdk = 36`，因此只修改 app 仍会请求不存在的 android-36 平台。

根 `build.gradle.kts` 现在通过 Android Components 的 `finalizeDsl`，将 Android library 模块统一设为 `compileSdk = 36`、`compileSdkMinor = 1`。这会使用已安装的 android-36.1，且不会修改 Cargo registry 内的依赖或 Tauri 自动生成文件。

已离线验证 `:tauri-android:compileDebugJavaWithJavac --dry-run --no-configuration-cache` 返回 `BUILD SUCCESSFUL`，任务依赖解析不再要求 android-36。此检查不执行实际 Java 编译或 APK 打包。

## `rustBuildArmDebug` 最后报找不到 `cargo.bat`

生成的 BuildTask 原来会在任意异常后依次重试 cargo / cargo.exe / cargo.cmd / cargo.bat。因此即使 Cargo 成功启动、只是 Rust 编译或 Tauri CLI 返回失败，最终也可能只看到找不到 cargo.bat。

BuildTask 现在优先使用 `deskpet.cargoExecutable` Gradle 属性、`CARGO` 环境变量，然后查找 `CARGO_HOME/bin` 或用户 `.cargo/bin` 中的可执行文件，最后才使用 PATH 上的 cargo.exe / cargo。使用绝对路径时也将该目录加入子进程 PATH，供 Cargo 查找 Tauri CLI。真正的编译失败会保留原始输出和退出码，不再尝试不存在的 cargo.bat。

自定义 Rust 安装位置时，可在本机 `~/.gradle/gradle.properties` 中设置（使用自己的实际路径）：

```properties
deskpet.cargoExecutable=C:/Users/Administrator/.cargo/bin/cargo.exe
```

已验证默认路径能找到本机 cargo.exe，以及使用测试进程检查 CLI 参数、工作目录、子进程 PATH 和失败退出码保留。后续补齐下面的 SDK/NDK 和链接器配置后，真实的 `:app:rustBuildArmDebug` 任务已构建成功。

### Cargo 启动后仍返回退出码 1

完整 Gradle 日志曾显示 `failed to ensure Android environment: Skipping Android Studio command line tools installation`。原因是 IDE 未继承 PowerShell 中临时设置的 NDK_HOME；Gradle 自己的 sdk.dir 也不会自动转成 Cargo 子进程的 ANDROID_HOME。

BuildTask 现从环境变量或本机 `gen/android/local.properties` 读取 SDK/NDK 位置，并传递给 Cargo。`sdk.dir` 继续使用 Android Studio 的值；手动解压的 NDK 在同一文件中使用如下本机配置（此文件不应提交或跨机器复制）：

```properties
deskpet.ndkHome=D:/Program Files/Android/ndk/android-ndk-r30-windows/android-ndk-r30
```

复现还发现 Rust 链接阶段误用了 Windows 的 cc / MinGW，导致 `unrecognized option '--eh-frame-hdr'`。BuildTask 现按目标架构设置 NDK Clang 的 CARGO_TARGET_*_LINKER、CC_*、AR_*，使用与 app.minSdk 一致的 API 24；ARM32 的 Clang 前缀是 armv7a，而 Rust triple 是 armv7。

验证时主动移除了当前终端的 ANDROID_HOME / NDK_HOME / ANDROID_NDK_HOME，让 Gradle 从 local.properties 获取路径。真实执行 `:app:rustBuildArmDebug --no-configuration-cache` 返回 `BUILD SUCCESSFUL in 50s`，生成 `target/armv7-linux-androideabi/debug/libdeskpet_app_lib.so` 并接入 `app/src/main/jniLibs/armeabi-v7a`。这验证了原来失败的 Rust 原生库构建任务，尚未验证完整 APK 打包和设备启动。

JDK 21 对 Java source/target 8 的弃用提示只是警告，不是这次 Rust 任务失败的原因。

`ArmDebug` 是 32 位 ARM（armv7），`Arm64Debug` 才是 ARM64。本次复现中 Tauri 已自动安装 armv7-linux-androideabi 目标，ARM64 真机仍应优先选择 arm64Debug 变体。

## 安装后 WebView 显示加载失败

设备日志确认初始调试 APK 在请求电脑的 `http://10.20.27.76:1430/`。这是 `tauri android dev` 为静态 frontendDist 启动的临时开发服务器；设备无法访问该地址时，WebView 显示错误页。`Cannot redefine property` 出现在随后注入脚本的 chrome-error 页面中，是次生错误。

本项目使用静态 HTML 和图片，Android 平台的 Tauri 依赖现单独启用 `custom-protocol` feature。包括 debug APK 在内，Android WebView 会从内置资源加载 frontendDist，避免依赖电脑 IP 或临时服务器。Windows 依赖没有启用这个 Android 专用 feature；通过 Windows GUI feature graph 验证。Android 前端文件修改后需要重新构建 APK 才能更新，不再使用该开发服务器的前端热更新。

验证结果：`:app:assembleArm64Debug` 成功，修复后的 APK 已覆盖安装到当前连接的 ARM64 设备并启动。WebView 调试接口返回 title=DeskPet、ready=complete、六种情绪图全部预加载、active=idle、精灵元素 display=block；截图确认基尔兽显示正常。地址 `http://tauri.localhost/` 是 Tauri 的本地资源协议映射，不是之前的电脑 IP 开发服务器。

APK：`crates/deskpet-app/gen/android/app/build/outputs/apk/arm64/debug/app-arm64-debug.apk`。

参考：[AGP 9.2 兼容性](https://developer.android.com/build/releases/agp-9-2-0-release-notes)、[Tauri 使用 Android Studio](https://v2.tauri.app/develop/#using-xcode-or-android-studio)。

## Android 气泡与模型设置

Android 使用单个 WebView，气泡由 `frontend/mobile.js` 在宠物页面内显示；原先页面只监听宠物情绪，没有监听 `bubble:show`，因此触发后看不到内容。现在双击基尔兽（两次触摸间隔小于 400ms）或点击底部“聊一条热点”都会立即显示等待气泡，随后展示播报结果。真实热点支持“喜欢”“不感兴趣”和关闭反馈。主动播报的完整 Agent 周期限制为 45 秒；网络或模型无响应时显示失败提示并恢复按钮。

右上角“设置”打开模型配置：

1. 填写服务商提供的兼容 OpenAI 的 API 地址、API Key 和模型名称。
2. 服务商不支持 embeddings 接口时，清空“向量模型”。
3. 点击“保存设置”，新配置立即应用到 LLM 和 Embedding，无需重启。

API Key 保存于 Android 应用私有目录的 `config.toml`，不会回传到前端输入框。已配置时留空会保留原密钥；输入新值会替换；勾选“清除已保存的 API Key”后保存会清除。关闭设置会清空密码输入框。其他采集、播报及 TTS 配置保持原值。

Windows 继续使用原有独立气泡、桌面拖动和托盘菜单，移动端界面仅在移动设备启用。

这些前端资源内置于 APK，修改后需要在 Android Studio 重新编译并安装 `arm64Debug`（ARM64 设备）。本轮初始 APK 编译和安装成功，真机双击已验证出现气泡；后续加入完整周期超时处理的最终 APK 尚未重新编译，因该次编译授权被拒绝。设置保存与密钥处理由本地单元测试覆盖，尚未完成真机设置保存验证。

### Android 沉浸式全屏

`app/src/main/java/com/deskpet/app/MainActivity.kt` 在创建、恢复前台和获得窗口焦点时，通过 AndroidX `WindowInsetsControllerCompat` 隐藏状态栏和底部导航栏。用户从屏幕边缘滑动可临时呼出系统栏，随后系统自动隐藏；仅仅 `enableEdgeToEdge()` 不会隐藏这些栏。

该改动只属于 Android Activity，不影响 Windows 宠物窗口或新增的托盘设置窗口。重新编译安装后生效，当前尚未完成新 Kotlin 代码编译和全屏真机验证：沙箱无法加载 Gradle 原生库，沙箱外编译授权被拒绝。

参考：[Android 沉浸模式官方文档](https://developer.android.com/develop/ui/views/layout/immersive)。

### 未配置 Agent API 与情绪恢复

未配置 API Key 时不再向模型发送请求，已有真实热点直接使用本地模板组成文案。双击或「聊一条热点」属于手动召唤，跳过自动播报评分门槛，仍然排除最近被标记为不感兴趣的内容。自动播报继续使用评分门槛与勿扰策略。手动召唤不推进自动轮询的候选游标。

模板模式仍依赖联网采集热点；热点库没有候选时不会编造内容，气泡会说明可能尚在采集或候选已被标记不感兴趣。

播报和反馈引起的临时情绪持续 8 秒后恢复 idle；新的情绪会替换旧恢复计时器。Android 关闭气泡或请求失败会立即恢复 idle，无候选时也保持 idle，避免永久停留在 curious。

回归验证命令：`cargo test -p deskpet-agent --offline`（17 项通过），`node scripts/test-pet-ui.cjs`（双击、重复触发保护、8 秒恢复、新旧计时器替换、关闭、失败、空结果）。前端资源和 Rust 库都已修改，需要重新编译安装 APK 后才会更新设备上的行为。

### 气泡位置调整

Android 已移除底部「聊一条热点」按钮，保留双击宠物触发。宠物、双击提示、气泡按上下顺序排布，气泡在宠物下方展开，长内容不会向上覆盖宠物。主页面固定在视口内，禁止整页上下滚动；气泡压缩到剩余可用高度，超长内容仅在气泡内部滚动，滚动到边界时不会带动页面。小屏或横屏适当缩小宠物以保留气泡空间；设置弹窗保留内部滚动。该改动只影响移动端页面，重新编译安装 APK 后生效。

### 双击实时刷新与触摸高亮

双击现在走独立的联网刷新流程：先显示刷新提示，并行请求已启用的采集源，跳过本地定时采集冷却。每个源最多等待 20 秒，刷新与生成回复的整体流程最多等待 45 秒；平台自身返回的限流或错误仍按失败处理。

Agent 只从本次成功请求返回的热点 ID 中选择候选，不从旧缓存中补选。数据库新增 announcements 表记录已经生成播报的热点，自动和手动播报均排除这些条目，重启后继续生效。刷新失败时显示失败原因，所有候选已播报时显示「本次刷新暂无未播报的新热点」，不会重新展示上次消息。未配置 API Key 时仍使用真实热点生成模板文案。

移动端使用透明的 `-webkit-tap-highlight-color`，禁用宠物区域长按菜单及选择高亮，保留气泡文本选择和设置表单的键盘焦点提示。

验证：Windows GUI 编译检查通过；Agent、采集、存储合计 41 项测试及宠物/气泡前端逻辑回归通过。本轮未重新打包 APK，也未做设备联网及触摸显示验证，需要重新编译安装后检查。
