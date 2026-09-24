package io.github.joycx.schemgen.common.config;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParseException;
import com.google.gson.JsonParser;
import io.github.joycx.schemgen.common.Json;
import io.github.joycx.schemgen.common.files.AtomicFiles;
import java.io.IOException;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.function.Consumer;

/**
 * {@code <config dir>/schemgen.json}. Every field has a working default, so a
 * missing file, a missing field or a file from an older version all load.
 * Blank paths mean "the default for this game instance".
 */
public final class ModConfig {
    public static final String FILE_NAME = "schemgen.json";
    public static final int DEFAULT_PORT = 3001;
    public static final int DEFAULT_PREVIEW_MAX_SIZE = 64;
    /** The server draws previews at 256 blocks at most. */
    public static final int PREVIEW_MAX_SIZE_LIMIT = 256;

    public ServerMode serverMode = ServerMode.SIDECAR;

    /** External mode: where the player's own server listens. */
    public String host = "127.0.0.1";

    public int port = DEFAULT_PORT;

    /** External mode: the server's {@code --token}, if it was started with one. */
    public String token = "";

    /** Sidecar mode: a schemgen2 binary to run instead of the bundled or downloaded one. */
    public String binaryPath = "";

    /**
     * Sidecar mode: the Python interpreter with trimesh the current server
     * still voxelizes through, passed as {@code SCHEMGEN_PYTHON}. Goes away
     * with the Rust voxelizer.
     */
    public String pythonPath = "";

    /** Where finished schematics go; blank is the instance's {@code schematics} folder. */
    public String outputFolder = "";

    /** The folder the model list shows; blank is {@code <game dir>/schemgen/models}. */
    public String modelsFolder = "";

    /** A target version to convert for instead of the running game's; blank follows the game. */
    public String targetOverride = "";

    /** The settings used last, restored into the form (see {@code SettingsModel.snapshot}). */
    public JsonObject lastSettings = new JsonObject();

    /** Place each finished schematic in Litematica, when it is installed. */
    public boolean autoLoadIntoLitematica = true;

    /** Longest side of a preview, in blocks. */
    public int previewMaxSize = DEFAULT_PREVIEW_MAX_SIZE;

    /**
     * Read the config. A file that cannot be parsed is kept aside as
     * {@code schemgen.json.broken} — so edits are not silently lost — and the
     * defaults are used.
     */
    public static ModConfig load(Path file, Consumer<String> warn) {
        if (!Files.isRegularFile(file)) {
            return new ModConfig();
        }
        try {
            JsonElement tree = JsonParser.parseString(Files.readString(file, StandardCharsets.UTF_8));
            if (!tree.isJsonObject()) {
                throw new JsonParseException("not a JSON object");
            }
            // An explicit null means "the default", as a missing field does.
            JsonObject fields = tree.getAsJsonObject();
            fields.entrySet().removeIf(e -> e.getValue().isJsonNull());
            return Json.PRETTY.fromJson(fields, ModConfig.class).sanitized();
        } catch (IOException | JsonParseException e) {
            Path aside = file.resolveSibling(file.getFileName() + ".broken");
            warn.accept("Could not read " + file + " (" + e.getMessage() + "); using defaults, the file is now " + aside);
            try {
                AtomicFiles.move(file, aside);
            } catch (IOException ignored) {
                // Nothing else to do: the defaults are used either way.
            }
            return new ModConfig();
        }
    }

    public void save(Path file) throws IOException {
        AtomicFiles.writeString(file, Json.PRETTY.toJson(sanitized()) + "\n");
    }

    /** This config with nulls and out-of-range numbers replaced by defaults. */
    public ModConfig sanitized() {
        ModConfig defaults = new ModConfig();
        ModConfig out = new ModConfig();
        out.serverMode = serverMode == null ? defaults.serverMode : serverMode;
        out.host = orDefault(host, defaults.host);
        out.port = port >= 1 && port <= 65535 ? port : defaults.port;
        out.token = orDefault(token, "");
        out.binaryPath = orDefault(binaryPath, "");
        out.pythonPath = orDefault(pythonPath, "");
        out.outputFolder = orDefault(outputFolder, "");
        out.modelsFolder = orDefault(modelsFolder, "");
        out.targetOverride = orDefault(targetOverride, "");
        out.lastSettings = lastSettings == null ? new JsonObject() : lastSettings.deepCopy();
        out.autoLoadIntoLitematica = autoLoadIntoLitematica;
        out.previewMaxSize = Math.max(1, Math.min(PREVIEW_MAX_SIZE_LIMIT, previewMaxSize));
        return out;
    }

    public Path outputFolder(Path gameDir) {
        return outputFolder.isBlank() ? gameDir.resolve("schematics") : Path.of(outputFolder.strip());
    }

    public Path modelsFolder(Path gameDir) {
        return modelsFolder.isBlank() ? gameDir.resolve("schemgen").resolve("models") : Path.of(modelsFolder.strip());
    }

    /** External mode's server address. */
    public URI externalUri() {
        String name = host.strip();
        if (name.contains(":") && !name.startsWith("[")) {
            name = "[" + name + "]"; // an IPv6 literal
        }
        return URI.create("http://" + name + ":" + port);
    }

    private static String orDefault(String value, String fallback) {
        return value == null || value.isBlank() ? fallback : value.strip();
    }
}
