import java.io.File
import java.util.Properties
import org.apache.tools.ant.taskdefs.condition.Os
import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.logging.LogLevel
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.TaskAction
import javax.inject.Inject
import org.gradle.process.ExecOperations

abstract class BuildTask : DefaultTask() {
    @get:Inject
    abstract val execOperations: ExecOperations

    @get:Input
    abstract val cargoExecutable: Property<String>

    @get:Input
    abstract val androidSdkDirectory: Property<String>

    @get:Input
    abstract val androidNdkDirectory: Property<String>

    @get:Input
    abstract val javaHomeDirectory: Property<String>

    init {
        val localProperties = Properties().apply {
            val localFile = project.rootProject.file("local.properties")
            if (localFile.isFile) localFile.inputStream().use { load(it) }
        }
        androidSdkDirectory.convention(
            project.providers.environmentVariable("ANDROID_HOME")
                .orElse(localProperties.getProperty("sdk.dir", ""))
        )
        androidNdkDirectory.convention(
            project.providers.environmentVariable("NDK_HOME")
                .orElse(project.providers.environmentVariable("ANDROID_NDK_HOME"))
                .orElse(localProperties.getProperty("deskpet.ndkHome", ""))
        )
        javaHomeDirectory.convention(
            project.providers.environmentVariable("JAVA_HOME")
                .orElse(project.providers.systemProperty("java.home"))
        )
        val cargoName = if (Os.isFamily(Os.FAMILY_WINDOWS)) "cargo.exe" else "cargo"
        val candidates = listOfNotNull(
            System.getenv("CARGO_HOME")?.let { File(it, "bin/$cargoName") },
            System.getProperty("user.home")?.let { File(it, ".cargo/bin/$cargoName") },
        )
        val installedCargo = candidates.firstOrNull { it.isFile }?.absolutePath ?: cargoName
        cargoExecutable.convention(
            project.providers.gradleProperty("deskpet.cargoExecutable")
                .orElse(project.providers.environmentVariable("CARGO"))
                .orElse(installedCargo)
        )
    }

    @Input
    var rootDirRel: String? = null
    @Input
    var projectDir: String? = null
    @Input
    var target: String? = null
    @Input
    var release: Boolean? = null

    @TaskAction
    fun assemble() {
        // A failed Rust build must retain its original error. Retrying every
        // failure as cargo.cmd/cargo.bat used to mask real compiler/CLI errors.
        runTauriCli(cargoExecutable.get())
    }

    fun runTauriCli(executable: String) {
        val rootDirRel = rootDirRel ?: throw GradleException("rootDirRel cannot be null")
        val target = target ?: throw GradleException("target cannot be null")
        val release = release ?: throw GradleException("release cannot be null")
        val projectDir = projectDir ?: throw GradleException("projectDir cannot be null")
        val args = listOf("tauri", "android", "android-studio-script");

        execOperations.exec {
            workingDir(File(projectDir, rootDirRel))
            executable(executable)
            androidSdkDirectory.get().takeIf { it.isNotBlank() }?.let {
                environment("ANDROID_HOME", it)
            }
            androidNdkDirectory.get().takeIf { it.isNotBlank() }?.let {
                environment("NDK_HOME", it)
                val (triple, clangPrefix) = when (target) {
                    "aarch64" -> "aarch64-linux-android" to "aarch64-linux-android"
                    "armv7" -> "armv7-linux-androideabi" to "armv7a-linux-androideabi"
                    "i686" -> "i686-linux-android" to "i686-linux-android"
                    "x86_64" -> "x86_64-linux-android" to "x86_64-linux-android"
                    else -> throw GradleException("Unsupported Rust Android target: $target")
                }
                val isWindows = Os.isFamily(Os.FAMILY_WINDOWS)
                val host = when {
                    isWindows -> "windows-x86_64"
                    Os.isFamily(Os.FAMILY_MAC) -> "darwin-x86_64"
                    else -> "linux-x86_64"
                }
                val ndkBin = File(it, "toolchains/llvm/prebuilt/$host/bin")
                // Match app.defaultConfig.minSdk = 24. The ARM32 Clang prefix
                // differs from the Rust triple (armv7a vs armv7).
                val clang = File(ndkBin, "$clangPrefix" + "24-clang" + if (isWindows) ".cmd" else "")
                if (!clang.isFile) {
                    throw GradleException("Android NDK compiler not found: $clang")
                }
                val targetKey = triple.replace('-', '_')
                environment("CARGO_TARGET_${targetKey.uppercase()}_LINKER", clang.absolutePath)
                environment("CC_$targetKey", clang.absolutePath)
                environment("AR_$targetKey", File(ndkBin, "llvm-ar" + if (isWindows) ".exe" else "").absolutePath)
            }
            javaHomeDirectory.get().takeIf { it.isNotBlank() }?.let {
                environment("JAVA_HOME", it)
            }
            // Android Studio may have inherited PATH before Rust was installed.
            // Cargo also needs to find the cargo-tauri executable beside it.
            val cargoBin = File(executable).takeIf { it.isAbsolute }?.parentFile
            if (cargoBin != null) {
                val oldPath = environment.entries
                    .firstOrNull { it.key.equals("PATH", ignoreCase = true) }
                    ?.value?.toString().orEmpty()
                environment("PATH", cargoBin.absolutePath + File.pathSeparator + oldPath)
            }
            args(args)
            if (logger.isEnabled(LogLevel.DEBUG)) {
                args("-vv")
            } else if (logger.isEnabled(LogLevel.INFO)) {
                args("-v")
            }
            if (release) {
                args("--release")
            }
            args(listOf("--target", target))
        }.assertNormalExitValue()
    }
}
