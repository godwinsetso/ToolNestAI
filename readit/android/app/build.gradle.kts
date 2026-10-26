plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "app.readit"
    compileSdk = 35

    defaultConfig {
        applicationId = "app.readit"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        ndk {
            // Phones and tablets (arm64, older arm) plus the emulator (x86_64).
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64")
        }
    }

    signingConfigs {
        // Release signing comes from environment variables so no key is ever committed.
        create("release") {
            val store = System.getenv("READIT_KEYSTORE")
            if (store != null) {
                storeFile = file(store)
                storePassword = System.getenv("READIT_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("READIT_KEY_ALIAS")
                keyPassword = System.getenv("READIT_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            if (System.getenv("READIT_KEYSTORE") != null) signingConfig = signingConfigs.getByName("release")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures { compose = true }

    packaging {
        // Extract .so files so libpdfium can be loaded by path from nativeLibraryDir.
        jniLibs.useLegacyPackaging = true
        resources.excludes += listOf("META-INF/{AL2.0,LGPL2.1}", "META-INF/DEPENDENCIES", "META-INF/LICENSE*", "META-INF/NOTICE*")
    }
}

kotlin {
    compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) }
}

dependencies {
    // sherpa-onnx (speech engine) is pinned to 1.13.8: 2.0.0 drops words missing from Kokoro's lexicon.
    // scripts/fetch-android-deps.sh downloads it into app/libs.
    implementation(files("libs/sherpa-onnx-1.13.8.aar"))
    implementation("${libs.jna.get()}@aar")

    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.activity.compose)
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.tooling.preview)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.material.icons)
    implementation(libs.androidx.navigation.compose)
    implementation(libs.androidx.media3.session)
    implementation(libs.androidx.media3.common)
    implementation(libs.androidx.work.runtime)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.coroutines.guava)
    implementation(libs.commons.compress)
    debugImplementation(libs.androidx.compose.ui.tooling)
    testImplementation(libs.junit)
}
