plugins { id("com.android.application"); id("org.jetbrains.kotlin.android") }
android {
    namespace = "it.ckiikc.musiccraft"
    compileSdk = 36
    defaultConfig {
        applicationId = "it.ckiikc.musiccraft"
        minSdk = 26
        targetSdk = 36
        versionCode = 10
        versionName = "0.4.1"
        testInstrumentationRunner = "it.ckiikc.musiccraft.MusicCraftInstrumentation"
    }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures { buildConfig = true }
    packaging { resources.excludes += "META-INF/versions/9/OSGI-INF/MANIFEST.MF" }
    lint { abortOnError = true }
}
dependencies {
    implementation("androidx.car.app:app:1.7.0")
    implementation("org.bouncycastle:bcprov-jdk18on:1.83")
    implementation("androidx.media3:media3-exoplayer:1.11.1")
    implementation("androidx.media3:media3-session:1.11.1")
    testImplementation("junit:junit:4.13.2")
}

val buildRatingTags by tasks.registering(Exec::class) {
    workingDir(rootProject.projectDir)
    commandLine("python3", "scripts/build-rating-tags.py")
    inputs.dir(rootProject.file("../crates/musiccraft-tags/src"))
    inputs.file(rootProject.file("../crates/musiccraft-tags/Cargo.toml"))
    inputs.file(rootProject.file("../crates/musiccraft-tags/Cargo.lock"))
    inputs.file(rootProject.file("scripts/build-rating-tags.py"))
    outputs.dir(rootProject.file("app/src/main/jniLibs"))
}
tasks.named("preBuild").configure { dependsOn(buildRatingTags) }
