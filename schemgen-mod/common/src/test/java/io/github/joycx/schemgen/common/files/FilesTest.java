package io.github.joycx.schemgen.common.files;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.stream.Stream;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class FilesTest {
    @TempDir
    Path dir;

    @Test
    void modelsAreGltfFilesSortedByName() throws IOException {
        Files.writeString(dir.resolve("b.glb"), "");
        Files.writeString(dir.resolve("A.GLTF"), "");
        Files.writeString(dir.resolve("notes.txt"), "");
        Files.createDirectories(dir.resolve("folder.glb"));
        assertEquals(List.of(dir.resolve("A.GLTF"), dir.resolve("b.glb")), ModelFiles.list(dir));
        assertEquals(List.of(), ModelFiles.list(dir.resolve("missing")));
    }

    @Test
    void freeNamesNeverOverwrite() throws IOException {
        assertEquals(dir.resolve("castle.litematic"), AtomicFiles.freeName(dir, "castle.litematic"));
        Files.writeString(dir.resolve("castle.litematic"), "");
        Files.writeString(dir.resolve("castle-2.litematic"), "");
        assertEquals(dir.resolve("castle-3.litematic"), AtomicFiles.freeName(dir, "castle.litematic"));
        assertEquals(dir.resolve("README"), AtomicFiles.freeName(dir, "README"));
    }

    @Test
    void atomicWritesLeaveTheOldFileOnFailure() throws IOException {
        Path target = dir.resolve("config.json");
        AtomicFiles.writeString(target, "old");
        assertThrows(IOException.class, () -> AtomicFiles.write(target, out -> {
            out.write('n');
            throw new IOException("disk full");
        }));
        assertEquals("old", Files.readString(target));
        try (Stream<Path> files = Files.list(dir)) {
            assertEquals(List.of(target), files.toList(), "no temporary file left");
        }
    }
}
