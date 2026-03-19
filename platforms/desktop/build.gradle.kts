import org.jetbrains.compose.desktop.application.dsl.TargetFormat

plugins {
    kotlin("jvm") version "2.0.21"
    id("org.jetbrains.compose") version "1.7.3"
    id("org.jetbrains.kotlin.plugin.compose") version "2.0.21"
}

group = "com.zipherx.wallet"
version = "1.1.0"

repositories {
    mavenCentral()
    maven("https://maven.pkg.jetbrains.space/public/p/compose/dev")
    google()
}

dependencies {
    // Compose Multiplatform Desktop
    implementation(compose.desktop.currentOs)
    implementation(compose.material3)
    implementation(compose.materialIconsExtended)

    // Coroutines
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.9.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-swing:1.9.0")

    // JNA (required by UniFFI-generated bindings)
    implementation("net.java.dev.jna:jna:5.14.0")

    // OS keyring access
    implementation("com.github.javakeyring:java-keyring:1.0.4")
}

compose.desktop {
    application {
        mainClass = "com.zipherx.wallet.MainKt"
        jvmArgs += listOf("-Djna.library.path=${project.projectDir}/src/main/resources")

        nativeDistributions {
            targetFormats(TargetFormat.Dmg, TargetFormat.Msi, TargetFormat.Deb, TargetFormat.Rpm)
            packageName = "ZipherX"
            packageVersion = "1.1.0"
            description = "ZipherX — Privacy-first Zclassic wallet"
            vendor = "ZipherX"

            linux {
                iconFile.set(project.file("src/main/resources/icon.png"))
            }
            windows {
                iconFile.set(project.file("src/main/resources/icon.ico"))
                menuGroup = "ZipherX"
                upgradeUuid = "a1b2c3d4-e5f6-7890-abcd-ef0123456789"
            }
            macOS {
                iconFile.set(project.file("src/main/resources/icon.icns"))
                bundleID = "com.zipherx.wallet"
            }
        }
    }
}
