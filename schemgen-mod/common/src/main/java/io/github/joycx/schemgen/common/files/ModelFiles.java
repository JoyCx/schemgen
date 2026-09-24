package io.github.joycx.schemgen.common.files;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Comparator;
import java.util.List;
import java.util.Locale;
import java.util.stream.Stream;

/** The models folder: glTF files the player dropped there, listed in the game. */
public final class ModelFiles {
    private ModelFiles() {}

    /** The extensions the server converts — the same test it applies to uploads. */
    public static boolean isModel(Path file) {
        String name = file.getFileName().toString().toLowerCase(Locale.ROOT);
        return name.endsWith(".glb") || name.endsWith(".gltf");
    }

    /** Models directly in {@code folder}, sorted by name; empty when the folder does not exist. */
    public static List<Path> list(Path folder) throws IOException {
        if (!Files.isDirectory(folder)) {
            return List.of();
        }
        try (Stream<Path> entries = Files.list(folder)) {
            return entries.filter(p -> Files.isRegularFile(p) && isModel(p))
                    .sorted(Comparator.comparing(p -> p.getFileName().toString().toLowerCase(Locale.ROOT)))
                    .toList();
        }
    }
}
