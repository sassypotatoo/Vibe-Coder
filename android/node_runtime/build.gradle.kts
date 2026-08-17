plugins {
    id("com.android.dynamic-feature")
}

android {
    namespace = "com.vibecoder.shell.node_runtime"
    compileSdk = 36
    ndkVersion = "28.2.13676358"

    defaultConfig {
        minSdk = 29
    }

    packaging {
        jniLibs {
            // The Node child-process payload must be extracted as a package-owned filesystem file.
            useLegacyPackaging = true
        }
    }

    sourceSets {
        getByName("main") {
            // A pinned, independently built Node artifact is staged here only for Play bundle builds.
            jniLibs.srcDir("build/generated/jniLibs")
            assets.srcDir("build/generated/assets")
        }
    }
}

dependencies {
    implementation(project(":app"))
}
