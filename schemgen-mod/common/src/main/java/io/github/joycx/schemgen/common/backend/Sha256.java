package io.github.joycx.schemgen.common.backend;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.HexFormat;
import java.util.Locale;

/** SHA-256 digests as lowercase hex — the form checksums are pinned in. */
public final class Sha256 {
    private Sha256() {}

    public static MessageDigest digest() {
        try {
            return MessageDigest.getInstance("SHA-256");
        } catch (NoSuchAlgorithmException e) {
            throw new IllegalStateException("Every Java runtime has SHA-256", e);
        }
    }

    public static String hex(MessageDigest digest) {
        return HexFormat.of().formatHex(digest.digest());
    }

    public static String of(byte[] bytes) {
        MessageDigest digest = digest();
        digest.update(bytes);
        return hex(digest);
    }

    public static String of(Path file) throws IOException {
        try (InputStream in = Files.newInputStream(file)) {
            MessageDigest digest = digest();
            byte[] buffer = new byte[64 * 1024];
            for (int n; (n = in.read(buffer)) != -1; ) {
                digest.update(buffer, 0, n);
            }
            return hex(digest);
        }
    }

    /** Whether {@code actual} is the pinned {@code expected} digest (hex, any case, surrounding space ignored). */
    public static boolean matches(String actual, String expected) {
        return expected != null && !expected.isBlank()
                && actual.toLowerCase(Locale.ROOT).equals(expected.strip().toLowerCase(Locale.ROOT));
    }
}
