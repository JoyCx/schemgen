package io.github.joycx.schemgen.common.backend;

import io.github.joycx.schemgen.common.files.AtomicFiles;
import java.io.IOException;
import java.io.InputStream;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.attribute.PosixFilePermission;
import java.time.Duration;
import java.util.Optional;
import java.util.Set;

/**
 * Finds the schemgen2 binary the sidecar runs, in order:
 *
 * <ol>
 *   <li>the path set in the config — the player's own build, run as is;
 *   <li>a binary bundled in the mod jar at {@code /bin/<os>-<arch>/schemgen2[.exe]},
 *       extracted to {@code <game dir>/schemgen/bin/<version>/};
 *   <li>the release asset for this platform, downloaded to the same folder;
 *   <li>otherwise none: the player has to run a server and use external mode.
 * </ol>
 *
 * Extracted and downloaded binaries must match the SHA-256 pinned in the jar
 * (see {@link ServerRelease}); anything else is deleted, never run.
 */
public final class ServerBinaries {
    /** The outcome: a binary to run, or why there is none. */
    public sealed interface Resolution permits Found, Unavailable {}

    /** {@code origin} is "configured", "bundled" or "downloaded". */
    public record Found(Path binary, String origin) implements Resolution {}

    /** {@code reason} is a sentence for the player. */
    public record Unavailable(String reason) implements Resolution {}

    /** Opens a resource of the mod jar, or returns {@code null} when there is none. */
    @FunctionalInterface
    public interface Resources {
        InputStream open(String path) throws IOException;
    }

    /** Fetches {@code uri} into {@code target}. */
    @FunctionalInterface
    public interface Downloader {
        void download(URI uri, Path target) throws IOException, InterruptedException;
    }

    private static final String EXTERNAL_HINT =
            " Start `schemgen2 serve` yourself and switch SchemGen to an external server.";

    private final ServerRelease release;
    private final Optional<Platform> platform;
    private final Resources resources;
    private final Downloader downloader;

    /** {@code release} is {@code null} when the jar pins none (a development build). */
    public ServerBinaries(ServerRelease release, Optional<Platform> platform, Resources resources, Downloader downloader) {
        this.release = release;
        this.platform = platform;
        this.resources = resources;
        this.downloader = downloader;
    }

    /** Resolution for the running JVM, with jar resources opened through {@code resources}. */
    public static ServerBinaries forCurrentPlatform(Resources resources) throws IOException {
        ServerRelease release;
        try (InputStream in = resources.open(ServerRelease.RESOURCE)) {
            release = in == null ? null : ServerRelease.read(in);
        }
        return new ServerBinaries(release, Platform.current(), resources, ServerBinaries::httpDownload);
    }

    public Resolution resolve(String configuredPath, Path gameDir) throws InterruptedException {
        if (configuredPath != null && !configuredPath.isBlank()) {
            Path binary = Path.of(configuredPath.strip());
            return Files.isRegularFile(binary)
                    ? new Found(binary, "configured")
                    : new Unavailable("The server binary set in the config does not exist: " + binary);
        }
        if (release == null) {
            return new Unavailable("This build of SchemGen pins no server release." + EXTERNAL_HINT);
        }
        if (platform.isEmpty()) {
            return new Unavailable("schemgen2 is not built for " + System.getProperty("os.name") + " on "
                    + System.getProperty("os.arch") + "." + EXTERNAL_HINT);
        }
        Platform p = platform.get();
        Path target = binFolder(gameDir).resolve(p.executableName());
        Optional<String> pinned = release.sha256(p);
        try {
            try (InputStream bundled = resources.open("/bin/" + p.key() + "/" + p.executableName())) {
                if (bundled != null) {
                    return extract(bundled, target, pinned, p);
                }
            }
            return download(target, pinned, p);
        } catch (IOException e) {
            return new Unavailable("Could not prepare the SchemGen server: " + e.getMessage());
        }
    }

    /** {@code <game dir>/schemgen/bin/<version>}: one folder per release, so an update never runs a stale binary. */
    public Path binFolder(Path gameDir) {
        return gameDir.resolve("schemgen").resolve("bin").resolve(release == null ? "dev" : release.version());
    }

    private Resolution extract(InputStream bundled, Path target, Optional<String> pinned, Platform p) throws IOException {
        if (pinned.isEmpty()) {
            return new Unavailable("The server bundled for " + p.key() + " has no pinned checksum, so it is not run."
                    + EXTERNAL_HINT);
        }
        if (verified(target, pinned.get())) {
            return new Found(target, "bundled");
        }
        Path temp = tempNextTo(target);
        try {
            Files.copy(bundled, temp, StandardCopyOption.REPLACE_EXISTING);
            if (!Sha256.matches(Sha256.of(temp), pinned.get())) {
                return new Unavailable("The server bundled in the mod does not match its pinned checksum, so it is "
                        + "not run. Reinstall SchemGen." + EXTERNAL_HINT);
            }
            install(temp, target);
            return new Found(target, "bundled");
        } finally {
            Files.deleteIfExists(temp);
        }
    }

    private Resolution download(Path target, Optional<String> pinned, Platform p)
            throws IOException, InterruptedException {
        Optional<URI> url = release.url(p);
        if (pinned.isEmpty() || url.isEmpty()) {
            return new Unavailable("No server for " + p.key() + " is bundled, and none can be downloaded safely "
                    + "because no checksum is pinned for it." + EXTERNAL_HINT);
        }
        if (verified(target, pinned.get())) {
            return new Found(target, "downloaded");
        }
        Path temp = tempNextTo(target);
        try {
            try {
                downloader.download(url.get(), temp);
            } catch (IOException e) {
                return new Unavailable("Could not download the SchemGen server from " + url.get() + ": "
                        + e.getMessage() + "." + EXTERNAL_HINT);
            }
            if (!Sha256.matches(Sha256.of(temp), pinned.get())) {
                return new Unavailable("The downloaded server does not match its pinned checksum and was deleted."
                        + EXTERNAL_HINT);
            }
            install(temp, target);
            return new Found(target, "downloaded");
        } finally {
            Files.deleteIfExists(temp);
        }
    }

    private static boolean verified(Path file, String pinned) throws IOException {
        return Files.isRegularFile(file) && Sha256.matches(Sha256.of(file), pinned);
    }

    private static Path tempNextTo(Path target) throws IOException {
        Files.createDirectories(target.getParent());
        return Files.createTempFile(target.getParent(), "." + target.getFileName(), ".part");
    }

    private static void install(Path verified, Path target) throws IOException {
        makeExecutable(verified);
        AtomicFiles.move(verified, target);
    }

    /** Owner read + execute on file systems with POSIX permissions; Windows needs nothing. */
    static void makeExecutable(Path file) throws IOException {
        try {
            Set<PosixFilePermission> permissions = Files.getPosixFilePermissions(file);
            permissions.add(PosixFilePermission.OWNER_READ);
            permissions.add(PosixFilePermission.OWNER_EXECUTE);
            Files.setPosixFilePermissions(file, permissions);
        } catch (UnsupportedOperationException notPosix) {
            // Windows: executability comes from the .exe name.
        }
    }

    /** GitHub release downloads redirect to a CDN; the system proxy settings apply. */
    private static void httpDownload(URI uri, Path target) throws IOException, InterruptedException {
        HttpClient http = HttpClient.newBuilder()
                .followRedirects(HttpClient.Redirect.NORMAL)
                .connectTimeout(Duration.ofSeconds(15))
                .build();
        try (http) {
            HttpRequest request = HttpRequest.newBuilder(uri).timeout(Duration.ofMinutes(5)).GET().build();
            HttpResponse<Path> response = http.send(request, HttpResponse.BodyHandlers.ofFile(target));
            if (response.statusCode() != 200) {
                throw new IOException("HTTP " + response.statusCode());
            }
        }
    }
}
