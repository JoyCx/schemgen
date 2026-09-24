package com.schemgen.mod;

import net.fabricmc.loader.api.FabricLoader;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;

/**
 * Where finished schematics land: {@code <game dir>/schematics}, the folder
 * Litematica reads.
 *
 * <p>The game directory comes from Fabric rather than the working directory,
 * so this is the running <em>instance's</em> folder — the per-instance one a
 * launcher created, not vanilla {@code .minecraft}.
 */
public final class SchematicsFolder {

    private SchematicsFolder() {}

    public static Path path() {
        return FabricLoader.getInstance().getGameDir().resolve("schematics");
    }

    /**
     * Write schematic bytes under {@code name}, never overwriting: a second
     * {@code castle} becomes {@code castle-2.litematic}, matching how the CLI
     * and the web UI de-duplicate.
     *
     * @return the file actually written
     */
    public static Path save(String name, byte[] contents) throws IOException {
        Path folder = path();
        Files.createDirectories(folder);

        String base = sanitize(name);
        Path target = folder.resolve(base + ".litematic");
        for (int suffix = 2; Files.exists(target); suffix++) {
            target = folder.resolve(base + "-" + suffix + ".litematic");
        }
        Files.write(target, contents);
        return target;
    }

    /**
     * Reduce a name to something every filesystem accepts. Path separators and
     * the Windows-reserved characters go, as does any leading dot, so a name
     * can neither escape the folder nor produce a hidden file.
     */
    public static String sanitize(String name) {
        String cleaned = name == null ? "" : name.trim();
        if (cleaned.toLowerCase().endsWith(".litematic")) {
            cleaned = cleaned.substring(0, cleaned.length() - ".litematic".length());
        }
        cleaned = cleaned.replaceAll("[\\\\/:*?\"<>|]", "_")
                .replaceAll("^[.\\s]+", "")
                .trim();
        if (cleaned.length() > 96) {
            cleaned = cleaned.substring(0, 96).trim();
        }
        return cleaned.isEmpty() ? "schemgen" : cleaned;
    }
}
