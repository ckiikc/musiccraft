plugins { id("com.android.application"); id("org.jetbrains.kotlin.android") }
android {
    namespace = "it.ckiikc.musiccraft"
    compileSdk = 36
    defaultConfig {
        applicationId = "it.ckiikc.musiccraft"
        minSdk = 26
        targetSdk = 36
        versionCode = 4
        versionName = "0.3.0"
        testInstrumentationRunner = "it.ckiikc.musiccraft.MusicCraftInstrumentation"
    }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures { buildConfig = true }
    lint { abortOnError = true }
}
dependencies {
    implementation("org.bouncycastle:bcprov-jdk18on:1.83")
    implementation("androidx.media3:media3-exoplayer:1.11.1")
    implementation("androidx.media3:media3-session:1.11.1")
    testImplementation("junit:junit:4.13.2")
}
