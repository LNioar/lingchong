plugins {
    `kotlin-dsl`
}

gradlePlugin {
    plugins {
        create("pluginsForCoolKids") {
            id = "rust"
            implementationClass = "RustPlugin"
        }
    }
}

repositories {
    // buildSrc is a separate build and does not inherit the root repositories.
    maven { url = uri("https://maven.aliyun.com/repository/google") }
    maven { url = uri("https://maven.aliyun.com/repository/central") }
}

dependencies {
    compileOnly(gradleApi())
    implementation("com.android.tools.build:gradle:9.2.1")
}

