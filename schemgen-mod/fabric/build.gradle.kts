import java.security.MessageDigest
import java.util.jar.Attributes
import java.util.jar.Manifest
import java.util.zip.ZipEntry
import java.util.zip.ZipInputStream
import java.util.zip.ZipOutputStream

// The Minecraft layer, built once per game version. Stonecutter runs this
// script for every fabric/versions/<version> project, with that folder's
// gradle.properties, and gives it fabric/src processed for that version.
plugins {
    id("fabric-loom")
}

val minecraftVersion: String = stonecutter.current.version

version = "${property("mod_version")}+mc$minecraftVersion"
group = property("maven_group") as String
base.archivesName = property("mod_id") as String

repositories {
    // Litematica and MaLiLib, only to compile the optional bridge against.
    exclusiveContent {
        forRepository { maven("https://api.modrinth.com/maven") { name = "Modrinth" } }
        filter { includeGroup("maven.modrinth") }
    }
}

/**
 * A mod to compile against only, as a copy whose manifest no longer names the
 * Loom that built it. Recent Litematica and MaLiLib builds are made with a
 * newer Loom than this build's (1.14 to 1.17 against 1.13, the newest on
 * Gradle 8), and Loom refuses to remap a jar stamped with a newer version of
 * itself. The stamp guards running such a jar, which the mod never does:
 * compiling against its classes is unaffected. Resolved while configuring,
 * because Loom sets up mod dependencies then.
 */
fun compileOnlyMod(notation: String): ConfigurableFileCollection {
    val jar = configurations.detachedConfiguration(dependencies.create(notation))
        .apply { isTransitive = false }
        .singleFile
    val copy = layout.buildDirectory.file("compile-only-mods/${jar.name}").get().asFile
    if (!copy.isFile || copy.lastModified() < jar.lastModified()) {
        copy.parentFile.mkdirs()
        ZipInputStream(jar.inputStream().buffered()).use { input ->
            ZipOutputStream(copy.outputStream().buffered()).use { output ->
                generateSequence { input.nextEntry }.forEach { entry ->
                    output.putNextEntry(ZipEntry(entry.name))
                    if (entry.name == "META-INF/MANIFEST.MF") {
                        val manifest = Manifest(input)
                        manifest.mainAttributes.remove(Attributes.Name("Fabric-Loom-Version"))
                        manifest.write(output)
                    } else {
                        input.copyTo(output)
                    }
                    output.closeEntry()
                }
            }
        }
    }
    return files(copy)
}

dependencies {
    minecraft("com.mojang:minecraft:$minecraftVersion")
    mappings("net.fabricmc:yarn:${property("yarn_mappings")}:v2")
    modImplementation("net.fabricmc:fabric-loader:${property("loader_version")}")

    // The Fabric API modules the mod uses; players install all of Fabric API.
    val fabricApiVersion = property("fabric_api_version") as String
    for (module in listOf("fabric-key-binding-api-v1", "fabric-lifecycle-events-v1", "fabric-rendering-v1")) {
        modImplementation(fabricApi.module(module, fabricApiVersion))
    }

    // Optional at runtime: the bridge is only loaded when Litematica is installed.
    modCompileOnly(compileOnlyMod("maven.modrinth:litematica:${property("litematica_version")}"))
    modCompileOnly(compileOnlyMod("maven.modrinth:malilib:${property("malilib_version")}"))

    // Everything that does not touch Minecraft, nested in the jar. It uses the
    // game's own Gson.
    implementation(project(":common"))
    include(project(":common"))
}

tasks.withType<JavaCompile>().configureEach {
    options.release = 21
    options.encoding = "UTF-8"
}

/**
 * Writes `schemgen-server.properties` — the schemgen2 release this jar
 * launches — and, when given binaries, puts them in the jar under
 * `bin/<os>-<arch>/`. The contract is in docs/mod.md:
 *
 *   -PserverBinaries=<dir>   files named schemgen2-<os>-<arch>[.exe] to bundle
 *   -PserverChecksums=<file> sha256sum output for the release assets, pinned so
 *                            the mod can verify a download
 *
 * Without either, the properties carry the version and release URLs but no
 * checksums, so the mod refuses to download and points to external mode.
 */
abstract class BundleServerBinaries : DefaultTask() {
    @get:Input
    abstract val serverVersion: Property<String>

    @get:InputDirectory
    @get:Optional
    @get:PathSensitive(PathSensitivity.NAME_ONLY)
    abstract val binaries: DirectoryProperty

    @get:InputFile
    @get:Optional
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val checksums: RegularFileProperty

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @TaskAction
    fun bundle() {
        val out = outputDir.get().asFile
        out.deleteRecursively()
        out.mkdirs()
        val version = serverVersion.get()

        val pinned = mutableMapOf<String, String>()
        checksums.orNull?.asFile?.let { file ->
            for (line in file.readLines().map(String::trim).filter { it.isNotEmpty() && !it.startsWith("#") }) {
                val parts = line.split(Regex("\\s+"), limit = 2)
                require(parts.size == 2 && parts[0].matches(Regex("[0-9a-fA-F]{64}"))) {
                    "$file: not a sha256sum line: $line"
                }
                val asset = parts[1].removePrefix("*").substringAfterLast('/')
                PLATFORMS.firstOrNull { assetName(it) == asset }?.let { pinned[it] = parts[0].lowercase() }
            }
        }

        binaries.orNull?.asFile?.let { dir ->
            for (platform in PLATFORMS) {
                val asset = dir.resolve(assetName(platform))
                if (!asset.isFile) continue
                val sha256 = sha256(asset)
                check(pinned[platform] == null || pinned[platform] == sha256) {
                    "$asset does not match its checksum in ${checksums.get().asFile}"
                }
                pinned[platform] = sha256
                asset.copyTo(out.resolve("bin/$platform/${executableName(platform)}"))
            }
        }

        val lines = mutableListOf("# The schemgen2 release this jar launches (bundleServerBinaries).", "version=$version")
        for (platform in PLATFORMS) {
            lines += "url.$platform=https://github.com/JoyCx/schemgen/releases/download/v$version/${assetName(platform)}"
            lines += "sha256.$platform=${pinned[platform].orEmpty()}"
        }
        out.resolve("schemgen-server.properties").writeText(lines.joinToString("\n", postfix = "\n"))
    }

    companion object {
        val PLATFORMS = listOf("windows-x64", "windows-arm64", "macos-x64", "macos-arm64", "linux-x64", "linux-arm64")

        fun executableName(platform: String) = if (platform.startsWith("windows")) "schemgen2.exe" else "schemgen2"

        fun assetName(platform: String) = "schemgen2-$platform" + if (platform.startsWith("windows")) ".exe" else ""

        fun sha256(file: java.io.File): String =
            MessageDigest.getInstance("SHA-256").digest(file.readBytes()).joinToString("") { "%02x".format(it) }
    }
}

val bundleServerBinaries = tasks.register<BundleServerBinaries>("bundleServerBinaries") {
    group = "build"
    description = "Pins the schemgen2 release this jar launches and bundles its binaries when given."
    // Inside a task's block, property() would look up the task's own properties.
    serverVersion.set(providers.gradleProperty("schemgen_server_version"))
    providers.gradleProperty("serverBinaries").orNull?.let { binaries.set(file(it)) }
    providers.gradleProperty("serverChecksums").orNull?.let { checksums.set(file(it)) }
    outputDir.set(layout.buildDirectory.dir("generated/schemgen-server"))
}

val modProperties = mapOf(
    "version" to version.toString(),
    "minecraft" to property("minecraft_range") as String,
)

tasks.processResources {
    inputs.properties(modProperties)
    filesMatching("fabric.mod.json") { expand(modProperties) }
    from(bundleServerBinaries)
}

tasks.jar {
    from(rootDir.parentFile.resolve("LICENSE")) { rename { "LICENSE_schemgen" } }
}
