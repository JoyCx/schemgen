package io.github.joycx.schemgen.common.files;

import java.io.IOException;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;

/**
 * Writing files so a reader never sees half of one: the bytes go to a
 * temporary file in the same folder, which then replaces the target in one
 * move. Litematica may be listing the schematics folder while a download
 * lands in it, and a crash mid-write must not leave a broken config.
 */
public final class AtomicFiles {
    private AtomicFiles() {}

    @FunctionalInterface
    public interface Writer {
        void writeTo(OutputStream out) throws IOException;
    }

    public static void write(Path target, Writer writer) throws IOException {
        Path folder = target.toAbsolutePath().getParent();
        Files.createDirectories(folder);
        Path temp = Files.createTempFile(folder, "." + target.getFileName(), ".part");
        try {
            try (OutputStream out = Files.newOutputStream(temp)) {
                writer.writeTo(out);
            }
            move(temp, target);
        } finally {
            Files.deleteIfExists(temp);
        }
    }

    public static void writeString(Path target, String text) throws IOException {
        write(target, out -> out.write(text.getBytes(StandardCharsets.UTF_8)));
    }

    /** Move {@code source} over {@code target}, atomically where the file system can. */
    public static void move(Path source, Path target) throws IOException {
        try {
            Files.move(source, target, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
        } catch (AtomicMoveNotSupportedException e) {
            Files.move(source, target, StandardCopyOption.REPLACE_EXISTING);
        }
    }

    /**
     * {@code folder/fileName}, or {@code name-2.ext}, {@code name-3.ext}, … when
     * that is taken — a new conversion never overwrites a schematic already in
     * the folder, which may be a build in progress.
     */
    public static Path freeName(Path folder, String fileName) {
        Path candidate = folder.resolve(fileName);
        int dot = fileName.lastIndexOf('.');
        String stem = dot > 0 ? fileName.substring(0, dot) : fileName;
        String ext = dot > 0 ? fileName.substring(dot) : "";
        for (int n = 2; Files.exists(candidate); n++) {
            candidate = folder.resolve(stem + "-" + n + ext);
        }
        return candidate;
    }
}
