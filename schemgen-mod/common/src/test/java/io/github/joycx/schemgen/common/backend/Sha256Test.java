package io.github.joycx.schemgen.common.backend;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class Sha256Test {
    /** FIPS 180-2 test vector for "abc". */
    private static final String ABC = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    @Test
    void knownVectors(@TempDir Path dir) throws Exception {
        assertEquals(ABC, Sha256.of("abc".getBytes(StandardCharsets.US_ASCII)));
        assertEquals("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", Sha256.of(new byte[0]));
        Path file = Files.writeString(dir.resolve("abc"), "abc");
        assertEquals(ABC, Sha256.of(file));
    }

    @Test
    void pinnedChecksumsCompareWithoutCaseOrSpaces() {
        assertTrue(Sha256.matches(ABC, " " + ABC.toUpperCase() + "\n"));
        assertFalse(Sha256.matches(ABC, ""));
        assertFalse(Sha256.matches(ABC, null));
        assertFalse(Sha256.matches(ABC, ABC.substring(1) + "0"));
    }
}
