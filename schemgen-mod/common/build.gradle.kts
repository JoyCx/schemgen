// Everything in the mod that does not touch Minecraft: API v2 client, sidecar
// launcher, settings schema and model, preview decoding, config. Plain Java,
// so it is compiled and tested without Loom or a game.
plugins {
    `java-library`
}

group = property("maven_group") as String
version = property("mod_version") as String

repositories {
    mavenCentral()
}

dependencies {
    // Minecraft ships Gson and the mod uses that copy, so it is compile-only
    // here. 2.10.1 is the oldest Gson the supported game versions ship: code
    // that compiles against it runs on all of them.
    compileOnly("com.google.code.gson:gson:2.10.1")

    testImplementation("com.google.code.gson:gson:2.10.1")
    testImplementation(platform("org.junit:junit-bom:5.14.4"))
    testImplementation("org.junit.jupiter:junit-jupiter")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

tasks.withType<JavaCompile>().configureEach {
    options.release = 21
    options.encoding = "UTF-8"
    options.compilerArgs.addAll(listOf("-Xlint:all", "-Werror"))
}

tasks.test {
    useJUnitPlatform()
    // ServerIntegrationTest runs against a real schemgen2 only when these are
    // set; declaring them as inputs makes setting one re-run the tests.
    for (name in listOf("SCHEMGEN_BINARY", "SCHEMGEN_PYTHON")) {
        inputs.property(name, providers.environmentVariable(name).orElse(""))
    }
    // The GLB models the backend's own tests convert.
    systemProperty("schemgen.fixtures", rootDir.parentFile.resolve("backend/fixtures").path)
    testLogging {
        events("passed", "skipped", "failed")
        exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
    }
}
