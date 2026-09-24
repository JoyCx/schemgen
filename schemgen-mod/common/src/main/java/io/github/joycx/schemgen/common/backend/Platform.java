package io.github.joycx.schemgen.common.backend;

import java.util.Locale;
import java.util.Optional;

/**
 * An operating system and CPU pair schemgen2 is built for, named the way
 * release assets and bundled binaries are: {@code linux-x64},
 * {@code macos-arm64}, {@code windows-x64}, …
 */
public record Platform(String os, String arch) {
    /** The platform this JVM runs on, when schemgen2 can exist for it. */
    public static Optional<Platform> current() {
        return of(System.getProperty("os.name", ""), System.getProperty("os.arch", ""));
    }

    /** Map Java's {@code os.name} and {@code os.arch} to a platform. */
    public static Optional<Platform> of(String osName, String osArch) {
        String name = osName.toLowerCase(Locale.ROOT);
        String os = name.startsWith("windows") ? "windows"
                : name.startsWith("mac") || name.contains("darwin") ? "macos"
                : name.contains("linux") ? "linux"
                : null;
        String arch = switch (osArch.toLowerCase(Locale.ROOT)) {
            case "amd64", "x86_64", "x64" -> "x64";
            case "aarch64", "arm64" -> "arm64";
            default -> null;
        };
        return os == null || arch == null ? Optional.empty() : Optional.of(new Platform(os, arch));
    }

    /** {@code linux-x64} — the key in {@code schemgen-server.properties} and the folder under {@code bin/}. */
    public String key() {
        return os + "-" + arch;
    }

    /** {@code schemgen2} or {@code schemgen2.exe}. */
    public String executableName() {
        return "windows".equals(os) ? "schemgen2.exe" : "schemgen2";
    }

    /** The release asset name: {@code schemgen2-linux-x64}, {@code schemgen2-windows-x64.exe}. */
    public String assetName() {
        return "schemgen2-" + key() + ("windows".equals(os) ? ".exe" : "");
    }
}
