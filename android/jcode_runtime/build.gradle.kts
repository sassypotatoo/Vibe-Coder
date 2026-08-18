plugins {
    id("com.android.dynamic-feature")
}

android {
    namespace = "com.vibecoder.shell.jcode_runtime"
    compileSdk = 36
    ndkVersion = "28.2.13676358"

    defaultConfig {
        minSdk = 29
    }

    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDir("build/generated/jniLibs")
        }
    }
}

dependencies {
    implementation(project(":app"))
}
