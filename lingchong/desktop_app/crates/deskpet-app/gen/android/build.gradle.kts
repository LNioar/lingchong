import com.android.build.api.variant.LibraryAndroidComponentsExtension

buildscript {
    repositories {
        maven { url = uri("https://maven.aliyun.com/repository/google") }
        maven { url = uri("https://maven.aliyun.com/repository/central") }
    }
    dependencies {
        classpath("com.android.tools.build:gradle:9.2.1")
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:2.2.10")
    }
}

// Tauri's Android library defaults to android-36, but this machine has
// android-36.1. Apply this after each library's own DSL has been evaluated,
// without editing generated files or the shared Cargo registry sources.
subprojects {
    pluginManager.withPlugin("com.android.library") {
        extensions.configure<LibraryAndroidComponentsExtension> {
            finalizeDsl { android ->
                android.compileSdk = 36
                android.compileSdkMinor = 1
            }
        }
    }
}

allprojects {
    repositories {
        maven { url = uri("https://maven.aliyun.com/repository/google") }
        maven { url = uri("https://maven.aliyun.com/repository/central") }
    }
}

tasks.register("clean").configure {
    delete("build")
}

