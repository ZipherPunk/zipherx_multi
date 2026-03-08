plugins {
    id("com.android.application") version "8.7.3"
    id("org.jetbrains.kotlin.android") version "2.0.21"
    id("org.jetbrains.kotlin.plugin.compose") version "2.0.21"
}

android {
    namespace = "com.zipherx.wallet"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.zipherx.wallet"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "1.0"

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"

        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    signingConfigs {
        create("release") {
            storeFile = file(System.getenv("ZIPHERX_KEYSTORE_PATH") ?: "release.keystore")
            storePassword = System.getenv("ZIPHERX_KEYSTORE_PASSWORD") ?: ""
            keyAlias = System.getenv("ZIPHERX_KEY_ALIAS") ?: "zipherx"
            keyPassword = System.getenv("ZIPHERX_KEY_PASSWORD") ?: ""
        }
    }

    buildTypes {
        release {
            // SECURITY (KA-5): R8/ProGuard minification is DISABLED because:
            //  1. UniFFI-generated Kotlin bindings use reflection via JNA — R8
            //     strips classes/methods that appear unused, breaking FFI calls.
            //  2. JNA itself relies on reflective class loading that requires
            //     keep rules not yet validated for this project.
            //  3. Compose compiler plugins generate code that can be mis-optimized
            //     without precise keep annotations.
            //
            // For production distribution, enable minification (isMinifyEnabled = true)
            // and add the necessary ProGuard/R8 keep rules in proguard-rules.pro:
            //   -keep class uniffi.** { *; }
            //   -keep class com.sun.jna.** { *; }
            //   -dontwarn com.sun.jna.**
            // Then run a full regression test on all FFI entry points before release.
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
            signingConfig = signingConfigs.getByName("release")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }


}

dependencies {
    // Jetpack Compose BOM
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)

    // Compose UI
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")

    // Lifecycle and ViewModel
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.7")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.activity:activity-compose:1.9.3")

    // Coroutines
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.9.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")

    // Core Android
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.appcompat:appcompat:1.6.1")

    // JNA (required by UniFFI-generated bindings)
    implementation("net.java.dev.jna:jna:5.14.0@aar")

    // Biometric + Fragment (required by BiometricPrompt)
    implementation("androidx.biometric:biometric:1.1.0")
    implementation("androidx.fragment:fragment-ktx:1.6.2")

    // Security (EncryptedSharedPreferences)
    implementation("androidx.security:security-crypto:1.1.0-alpha06")

    // Process lifecycle (foreground detection)
    implementation("androidx.lifecycle:lifecycle-process:2.8.7")

    // Testing
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.5")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.1")
    androidTestImplementation(composeBom)
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    debugImplementation("androidx.compose.ui:ui-tooling")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}
