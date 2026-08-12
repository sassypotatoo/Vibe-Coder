plugins {
    id("com.android.application")
}

android {
    namespace = "com.vibecoder.shell"
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
        versionCode = 31
        versionName = "0.31.0"

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
