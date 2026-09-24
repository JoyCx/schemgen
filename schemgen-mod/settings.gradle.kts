// SchemGen, the Minecraft mod: an in-game client of the schemgen2 server.
//
// `common` is plain Java 21 — the HTTP client, the sidecar launcher, the
// settings schema and model — and builds and tests anywhere. `fabric` is the
// Minecraft layer: Stonecutter turns its one source tree into a Loom project
// per game version (`:fabric:1.21.8`, …).
pluginManagement {
    repositories {
        maven("https://maven.fabricmc.net/") { name = "Fabric" }
        gradlePluginPortal()
        mavenCentral()
    }
    plugins {
        // Loom 1.13 is the newest line that runs on Gradle 8.14 (1.14 needs
        // Gradle 9). It builds every 1.21.x version below, 1.21.11 included.
        id("fabric-loom") version "1.13.6"
    }
}

plugins {
    // 0.7.x is the newest Stonecutter line for Gradle 8 (0.8 and later need Gradle 9).
    id("dev.kikugie.stonecutter") version "0.7.11"
}

rootProject.name = "schemgen-mod"

include("common")

stonecutter {
    create(":fabric") {
        // Game versions whose APIs were checked against their Yarn mappings,
        // Fabric API and Litematica sources — see docs/mod.md before adding one.
        versions("1.21.1", "1.21.4", "1.21.8", "1.21.11")
        // The version `src/` is written for when committed.
        vcsVersion = "1.21.8"
    }
}
