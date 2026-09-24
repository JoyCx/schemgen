package com.schemgen.mod;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonSyntaxException;

import net.fabricmc.loader.api.FabricLoader;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;

/**
 * Settings kept in {@code config/schemgen.json}, so the server URL and the
 * conversion defaults survive a restart.
 *
 * <p>Field names are the JSON keys. A missing or malformed file is not an
 * error — the defaults are written back out and the mod carries on.
 */
public final class SchemGenConfig {

    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().create();

    /** Base URL of a running {@code schemgen2 serve}. No trailing slash. */
    public String serverUrl = "http://localhost:3001";

    /** Longest axis of the finished build, in blocks. */
    public int maxSize = 128;

    /** 8x8 Bayer ordered dithering. */
    public boolean dither = true;

    /** Lighting separation strength, 0 = off. See docs/pipeline.md. */
    public float delight = 0.0f;

    /** Folder the file picker opens in; empty means the OS default. */
    public String lastDirectory = "";

    public static SchemGenConfig defaults() {
        return new SchemGenConfig();
    }

    private static Path path() {
        return FabricLoader.getInstance().getConfigDir().resolve("schemgen.json");
    }

    public static SchemGenConfig load() {
        Path file = path();
        if (!Files.isRegularFile(file)) {
            SchemGenConfig fresh = defaults();
            fresh.save();
            return fresh;
        }
        try {
            String json = Files.readString(file, StandardCharsets.UTF_8);
            SchemGenConfig loaded = GSON.fromJson(json, SchemGenConfig.class);
            return loaded == null ? defaults() : loaded.sanitized();
        } catch (IOException | JsonSyntaxException e) {
            SchemGenMod.LOGGER.warn("Could not read {} ({}) — using defaults", file, e.toString());
            return defaults();
        }
    }

    public void save() {
        Path file = path();
        try {
            Files.createDirectories(file.getParent());
            Files.writeString(file, GSON.toJson(sanitized()), StandardCharsets.UTF_8);
        } catch (IOException e) {
            SchemGenMod.LOGGER.warn("Could not write {}: {}", file, e.toString());
        }
    }

    /** Clamp anything a hand-edited file could have put out of range. */
    private SchemGenConfig sanitized() {
        if (serverUrl == null || serverUrl.isBlank()) {
            serverUrl = "http://localhost:3001";
        }
        while (serverUrl.endsWith("/")) {
            serverUrl = serverUrl.substring(0, serverUrl.length() - 1);
        }
        maxSize = Math.clamp(maxSize, 16, 512);
        delight = Math.clamp(delight, 0.0f, 1.0f);
        if (lastDirectory == null) {
            lastDirectory = "";
        }
        return this;
    }
}
