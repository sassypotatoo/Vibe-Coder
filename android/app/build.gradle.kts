import org.gradle.api.tasks.compile.JavaCompile

plugins {
    id("com.android.application")
}

android {
    namespace = "com.vibecoder.shell"
    dynamicFeatures += setOf(":node_runtime")
    compileSdk = 36
    ndkVersion = "28.2.13676358"

    signingConfigs {
        create("diagnosticDebug") {
            storeFile = file("../signing/vibecoder-diagnostic-debug.jks")
            storePassword = "vibecoder-debug"
            keyAlias = "vibecoder-diagnostic"
            keyPassword = "vibecoder-debug"
            storeType = "JKS"
        }
    }

    defaultConfig {
        applicationId = "com.vibecoder.shell"
        minSdk = 29
        targetSdk = 36
        versionCode = 32
        versionName = "0.32.0"

        ndk {
            abiFilters += "arm64-v8a"
        }

        externalNativeBuild {
            cmake {
                arguments += listOf("-DANDROID_STL=none")
            }
        }
    }

    buildTypes {
        debug {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("diagnosticDebug")
        }
        release {
            isMinifyEnabled = false
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    // The OmniRoute runtime is independently sealed and verified before AAPT sees it. AAPT's
    // default generic hidden/directory/editor ignore rules are therefore unsafe:
    // they can silently delete legitimate Next.js runtime paths such as `.next` and `_not-found`,
    // breaking the manifest-bound tree after an otherwise successful APK build. Use one exact,
    // non-matching sentinel instead of any generic ignore rule. A pre-Gradle gate scans the staged
    // tree with AAPT-compatible matching and fails if this sentinel ever collides with a real path.
    androidResources.ignoreAssetsPattern = "__vibecoder_aapt_ignore_none__"

    // Part 27 needs child-process native payloads to exist as real package-owned filesystem files.
    // Legacy JNI packaging compresses .so entries in the APK and lets PackageManager extract them.
    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }

    // Runtime binaries are generated build outputs, never committed source files.
    // Keep them under build/ so source-integrity validation remains meaningful.
    sourceSets {
        getByName("main") {
            jniLibs.srcDir("build/generated/jniLibs")
            // Part 34.3.3: generated, verified OmniRoute production bundle only.
            // The reviewed source archive is never packaged into the APK.
            assets.srcDir("build/generated/omnirouteAssets")
        }
    }

    externalNativeBuild {
        cmake {
            path = file("src/main/cpp/CMakeLists.txt")
            version = "3.22.1"
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation("com.google.android.play:feature-delivery:2.1.0")
}

// Compile the real Android source against its resolved Play Feature Delivery dependency.
// Keep warnings fatal so the old standalone javac gate remains a real compile contract.
tasks.withType<JavaCompile>().configureEach {
    options.compilerArgs.addAll(listOf("-Xlint:all", "-Werror"))
}
