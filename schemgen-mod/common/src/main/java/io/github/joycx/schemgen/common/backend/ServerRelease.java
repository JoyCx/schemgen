package io.github.joycx.schemgen.common.backend;

import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.Reader;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.util.Optional;
import java.util.Properties;

/**
 * The schemgen2 release this build of the mod is pinned to, from the jar
 * resource {@code schemgen-server.properties} that the {@code bundleServerBinaries}
 * Gradle task writes:
 *
 * <pre>
 * version=2.0.0
 * sha256.linux-x64=&lt;hex&gt;
 * url.linux-x64=https://github.com/JoyCx/schemgen/releases/download/v2.0.0/schemgen2-linux-x64
 * </pre>
 *
 * A binary — bundled in the jar or downloaded — only ever runs when its
 * SHA-256 is the one pinned here.
 */
public record ServerRelease(String version, Properties properties) {
    public static final String RESOURCE = "/schemgen-server.properties";

    public static ServerRelease read(InputStream in) throws IOException {
        Properties properties = new Properties();
        try (Reader reader = new InputStreamReader(in, StandardCharsets.UTF_8)) {
            properties.load(reader);
        }
        String version = properties.getProperty("version", "").strip();
        if (version.isEmpty()) {
            throw new IOException("schemgen-server.properties names no version");
        }
        return new ServerRelease(version, properties);
    }

    /** The pinned checksum for a platform, if the build knew one. */
    public Optional<String> sha256(Platform platform) {
        return value("sha256." + platform.key());
    }

    /** Where the platform's binary is published — the GitHub release of {@link #version()}. */
    public Optional<URI> url(Platform platform) {
        return value("url." + platform.key()).map(URI::create);
    }

    private Optional<String> value(String key) {
        String value = properties.getProperty(key, "").strip();
        return value.isEmpty() ? Optional.empty() : Optional.of(value);
    }
}
