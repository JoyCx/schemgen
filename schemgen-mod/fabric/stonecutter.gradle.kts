// Stonecutter controller for the Fabric projects. `src/` is written for the
// active version; "Set active project to <version>" rewrites its version
// comments for another one, and every version builds from its own processed
// copy regardless (`./gradlew :fabric:1.21.1:build`).
plugins {
    id("dev.kikugie.stonecutter")
}

stonecutter active "1.21.8"
