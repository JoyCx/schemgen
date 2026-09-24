package io.github.joycx.schemgen.common.backend;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.ByteArrayInputStream;
import java.io.IOException;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.stream.Stream;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class ServerBinariesTest {
    private static final Platform LINUX = new Platform("linux", "x64");
    private static final byte[] BINARY = "#!/bin/sh\necho schemgen2\n".getBytes(StandardCharsets.UTF_8);

    @TempDir
    Path gameDir;

    private final Map<String, byte[]> jar = new HashMap<>();
    private final AtomicInteger downloads = new AtomicInteger();

    private ServerRelease release(String extra) throws IOException {
        String text = "version=2.0.0\n" + extra;
        return ServerRelease.read(new ByteArrayInputStream(text.getBytes(StandardCharsets.UTF_8)));
    }

    private ServerBinaries binaries(ServerRelease release, byte[] served) {
        ServerBinaries.Resources resources = path -> {
            byte[] bytes = jar.get(path);
            return bytes == null ? null : new ByteArrayInputStream(bytes);
        };
        ServerBinaries.Downloader downloader = (uri, target) -> {
            downloads.incrementAndGet();
            if (served == null) {
                throw new IOException("HTTP 404");
            }
            Files.write(target, served);
        };
        return new ServerBinaries(release, Optional.of(LINUX), resources, downloader);
    }

    private Path installed() {
        return gameDir.resolve("schemgen/bin/2.0.0/schemgen2");
    }

    @Test
    void aConfiguredBinaryIsUsedAsIs() throws Exception {
        Path own = Files.write(gameDir.resolve("my-schemgen2"), BINARY);
        ServerBinaries.Resolution r = binaries(release(""), null).resolve(own.toString(), gameDir);
        assertEquals(new ServerBinaries.Found(own, "configured"), r);

        ServerBinaries.Resolution missing = binaries(release(""), null).resolve("/no/such/schemgen2", gameDir);
        assertTrue(((ServerBinaries.Unavailable) missing).reason().contains("/no/such/schemgen2"));
    }

    @Test
    void aBundledBinaryIsExtractedOnceWhenItsChecksumMatches() throws Exception {
        jar.put("/bin/linux-x64/schemgen2", BINARY);
        ServerBinaries binaries = binaries(release("sha256.linux-x64=" + Sha256.of(BINARY).toUpperCase() + "\n"), null);

        assertEquals(new ServerBinaries.Found(installed(), "bundled"), binaries.resolve("", gameDir));
        assertArrayEquals(BINARY, Files.readAllBytes(installed()));
        assertTrue(Files.isExecutable(installed()));
        long firstWrite = Files.getLastModifiedTime(installed()).toMillis();

        jar.put("/bin/linux-x64/schemgen2", "changed".getBytes(StandardCharsets.UTF_8));
        // The verified copy on disk is reused rather than extracted again.
        assertEquals(new ServerBinaries.Found(installed(), "bundled"), binaries.resolve(null, gameDir));
        assertEquals(firstWrite, Files.getLastModifiedTime(installed()).toMillis());
        assertEquals(0, downloads.get());
    }

    @Test
    void aBundledBinaryThatDoesNotMatchIsNeverInstalled() throws Exception {
        jar.put("/bin/linux-x64/schemgen2", BINARY);
        ServerBinaries.Resolution r = binaries(release("sha256.linux-x64=" + "0".repeat(64) + "\n"), null)
                .resolve("", gameDir);

        assertTrue(((ServerBinaries.Unavailable) r).reason().contains("does not match its pinned checksum"));
        assertFalse(Files.exists(installed()));
        assertNoTemporaryFiles();
    }

    @Test
    void aBundledBinaryWithoutAChecksumIsRefused() throws Exception {
        jar.put("/bin/linux-x64/schemgen2", BINARY);
        assertInstanceOf(ServerBinaries.Unavailable.class, binaries(release(""), null).resolve("", gameDir));
        assertFalse(Files.exists(installed()));
    }

    @Test
    void withoutABundleTheReleaseIsDownloadedAndVerified() throws Exception {
        String pins = "sha256.linux-x64=" + Sha256.of(BINARY) + "\nurl.linux-x64=https://example.invalid/schemgen2-linux-x64\n";
        ServerBinaries binaries = binaries(release(pins), BINARY);

        assertEquals(new ServerBinaries.Found(installed(), "downloaded"), binaries.resolve("", gameDir));
        assertTrue(Files.isExecutable(installed()));
        assertEquals(new ServerBinaries.Found(installed(), "downloaded"), binaries.resolve("", gameDir));
        assertEquals(1, downloads.get(), "a verified download is kept");
    }

    @Test
    void aTamperedDownloadIsDeletedAndRefused() throws Exception {
        String pins = "sha256.linux-x64=" + Sha256.of(BINARY) + "\nurl.linux-x64=https://example.invalid/x\n";
        ServerBinaries.Resolution r = binaries(release(pins), "evil".getBytes(StandardCharsets.UTF_8)).resolve("", gameDir);

        String reason = ((ServerBinaries.Unavailable) r).reason();
        assertTrue(reason.contains("does not match its pinned checksum"), reason);
        assertTrue(reason.contains("external server"), "suggests external mode: " + reason);
        assertFalse(Files.exists(installed()));
        assertNoTemporaryFiles();
    }

    @Test
    void noChecksumMeansNoDownload() throws Exception {
        ServerBinaries.Resolution r = binaries(release("url.linux-x64=https://example.invalid/x\n"), BINARY)
                .resolve("", gameDir);
        assertTrue(((ServerBinaries.Unavailable) r).reason().contains("no checksum is pinned"));
        assertEquals(0, downloads.get());
    }

    @Test
    void aFailedDownloadSuggestsAnExternalServer() throws Exception {
        String pins = "sha256.linux-x64=" + Sha256.of(BINARY) + "\nurl.linux-x64=https://example.invalid/x\n";
        String reason = ((ServerBinaries.Unavailable) binaries(release(pins), null).resolve("", gameDir)).reason();
        assertTrue(reason.startsWith("Could not download the SchemGen server from https://example.invalid/x: HTTP 404"), reason);
        assertTrue(reason.contains("schemgen2 serve"), reason);
    }

    @Test
    void unsupportedPlatformsAndMissingPinsSayWhy() throws Exception {
        ServerBinaries unknownPlatform = new ServerBinaries(release(""), Optional.empty(), path -> null, (u, t) -> {});
        assertInstanceOf(ServerBinaries.Unavailable.class, unknownPlatform.resolve("", gameDir));
        ServerBinaries noRelease = new ServerBinaries(null, Optional.of(LINUX), path -> null, (u, t) -> {});
        assertTrue(((ServerBinaries.Unavailable) noRelease.resolve("", gameDir)).reason().contains("pins no server release"));
    }

    @Test
    void theJarResourceIsReadForTheCurrentPlatform() throws Exception {
        jar.put(ServerRelease.RESOURCE, "version=9.9.9\n".getBytes(StandardCharsets.UTF_8));
        ServerBinaries.Resources resources = path -> {
            byte[] bytes = jar.get(path);
            return bytes == null ? null : new ByteArrayInputStream(bytes);
        };
        ServerBinaries binaries = ServerBinaries.forCurrentPlatform(resources);
        assertEquals(gameDir.resolve("schemgen/bin/9.9.9"), binaries.binFolder(gameDir));
    }

    @Test
    void platformsAreNamedLikeReleaseAssets() {
        assertEquals(Optional.of(new Platform("linux", "x64")), Platform.of("Linux", "amd64"));
        assertEquals(Optional.of(new Platform("macos", "arm64")), Platform.of("Mac OS X", "aarch64"));
        assertEquals(Optional.of(new Platform("windows", "x64")), Platform.of("Windows 11", "amd64"));
        assertEquals(Optional.empty(), Platform.of("Linux", "x86"));
        assertEquals(Optional.empty(), Platform.of("FreeBSD", "amd64"));
        Platform windows = new Platform("windows", "x64");
        assertEquals("schemgen2-windows-x64.exe", windows.assetName());
        assertEquals("schemgen2.exe", windows.executableName());
        assertEquals("schemgen2-linux-x64", LINUX.assetName());
        assertEquals(Optional.of(URI.create("https://x/y")), releaseWithUrl().url(LINUX));
    }

    private ServerRelease releaseWithUrl() {
        try {
            return release("url.linux-x64 = https://x/y \n");
        } catch (IOException e) {
            throw new AssertionError(e);
        }
    }

    private void assertNoTemporaryFiles() throws IOException {
        Path folder = installed().getParent();
        if (Files.isDirectory(folder)) {
            try (Stream<Path> files = Files.list(folder)) {
                assertEquals(List.of(), files.toList());
            }
        }
    }
}
