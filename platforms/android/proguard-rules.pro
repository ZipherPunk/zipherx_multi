# SECURITY NOTE: These rules are intentionally broad because R8/ProGuard minification
# is currently disabled (isMinifyEnabled = false). When minification is enabled for
# production, narrow these rules to keep only the specific classes/methods required
# by UniFFI, JNA, and Compose reflection, and run full regression tests.

# Keep all wallet classes that interface with FFI
-keep class com.zipherx.wallet.** { *; }

# Keep UniFFI generated bindings
-keep class uniffi.** { *; }

# Keep JNA classes
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-dontwarn com.sun.jna.**

# Strip debug logging in release
-assumenosideeffects class android.util.Log {
    public static int d(...);
    public static int v(...);
    public static int i(...);
}

# Keep Compose runtime
-keep class androidx.compose.** { *; }
