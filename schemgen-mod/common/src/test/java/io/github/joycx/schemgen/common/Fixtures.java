package io.github.joycx.schemgen.common;

import io.github.joycx.schemgen.common.schema.Schema;
import java.io.IOException;
import java.io.InputStream;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;

/** Responses captured from a real schemgen2 2.1.0 ({@code src/test/resources/fixtures}). */
public final class Fixtures {
    private Fixtures() {}

    public static String text(String name) {
        try (InputStream in = Fixtures.class.getResourceAsStream("/fixtures/" + name)) {
            if (in == null) {
                throw new IllegalArgumentException("No fixture " + name);
            }
            return new String(in.readAllBytes(), StandardCharsets.UTF_8);
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }

    /** {@code GET /api/schema}. */
    public static Schema schema() {
        return Schema.parse(text("schema.json"));
    }
}
