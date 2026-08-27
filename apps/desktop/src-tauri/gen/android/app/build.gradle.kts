import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

android {
    compileSdk = 36
    namespace = "kr.devfive.hanbeon"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "kr.devfive.hanbeon"
        minSdk = 24
        targetSdk = 36
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            // Debug symbols made the universal APK 333MB, which repeatedly broke
            // installs over wireless ADB. Stripping them keeps the APK ~13MB;
            // native crash frames are still symbolized from target/ locally.
            packaging {
                jniLibs.keepDebugSymbols.clear()
            }
        }
        getByName("release") {
            isMinifyEnabled = true
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    buildFeatures {
        buildConfig = true
    }
}

rust {
    rootDirRel = "../../../"
}

// The Tauri rust plugin builds src-tauri (libhanbeon_lib.so) only. crates/hanbeon-jni
// is a separate crate that exports every Java_kr_devfive_hanbeon_Core_native* symbol
// Core.kt declares, and nothing rebuilt it: a stale libhanbeon_jni.so shipped with 5
// of 9 symbols and the app died at startup with UnsatisfiedLinkError. Build it here so
// a missing symbol can never reach a device again.
val jniAbis = mapOf(
    "arm64-v8a" to "aarch64-linux-android",
    "armeabi-v7a" to "armv7-linux-androideabi",
    "x86" to "i686-linux-android",
    "x86_64" to "x86_64-linux-android",
)

val buildHanbeonJni by tasks.registering {
    description = "Builds crates/hanbeon-jni for each Android ABI into app/src/main/jniLibs."
    val workspaceRoot = file("../../../../../../")
    val jniLibsDir = file("src/main/jniLibs")
    inputs.dir(File(workspaceRoot, "crates/hanbeon-jni/src"))
    inputs.dir(File(workspaceRoot, "crates/hanbeon-core/src"))
    outputs.dir(jniLibsDir)

    doLast {
        // Only ABIs already present are refreshed, so a single-target debug build
        // stays fast; a fresh ABI directory is created on demand for release builds.
        val requested = jniAbis.filterKeys { abi ->
            File(jniLibsDir, abi).exists() || project.hasProperty("hanbeonAllAbis")
        }
        require(requested.isNotEmpty()) { "no target ABI found under ${jniLibsDir}" }

        requested.forEach { (abi, target) ->
            providers.exec {
                workingDir = workspaceRoot
                commandLine("cargo", "build", "-p", "hanbeon-jni", "--release", "--target", target)
            }.result.get().assertNormalExitValue()

            val built = File(workspaceRoot, "target/$target/release/libhanbeon_jni.so")
            require(built.isFile) { "hanbeon-jni did not produce $built" }
            val destDir = File(jniLibsDir, abi).apply { mkdirs() }
            built.copyTo(File(destDir, "libhanbeon_jni.so"), overwrite = true)
        }
    }
}

tasks.matching { it.name.startsWith("merge") && it.name.endsWith("JniLibFolders") }
    .configureEach { dependsOn(buildHanbeonJni) }

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")