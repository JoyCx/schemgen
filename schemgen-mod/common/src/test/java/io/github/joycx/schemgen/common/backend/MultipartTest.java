package io.github.joycx.schemgen.common.backend;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.InvalidPathException;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class MultipartTest {
    @TempDir
    Path dir;

    @Test
    void bodyHasTheExactMultipartLayout() throws IOException {
        Path model = Files.write(dir.resolve("castle.glb"), new byte[] {0x67, 0x6c, 0x54, 0x46, 0, (byte) 0xff});
        Multipart body = new Multipart("BOUNDARY")
                .addFile("file", model, "application/octet-stream")
                .addText("settings", "application/json; charset=utf-8", "{\"max_size\":64}");

        ByteArrayOutputStream expected = new ByteArrayOutputStream();
        expected.writeBytes(("--BOUNDARY\r\n"
                + "Content-Disposition: form-data; name=\"file\"; filename=\"castle.glb\"\r\n"
                + "Content-Type: application/octet-stream\r\n"
                + "\r\n").getBytes(StandardCharsets.UTF_8));
        expected.writeBytes(Files.readAllBytes(model));
        expected.writeBytes(("\r\n"
                + "--BOUNDARY\r\n"
                + "Content-Disposition: form-data; name=\"settings\"\r\n"
                + "Content-Type: application/json; charset=utf-8\r\n"
                + "\r\n"
                + "{\"max_size\":64}\r\n"
                + "--BOUNDARY--\r\n").getBytes(StandardCharsets.UTF_8));

        assertArrayEquals(expected.toByteArray(), body.toBytes());
        assertEquals("multipart/form-data; boundary=BOUNDARY", body.contentType());
    }

    @Test
    void namesAreEscapedLikeBrowsers() throws IOException {
        Path model = Files.writeString(dir.resolve("castle \"v2\".glb"), "x");
        String text = new String(new Multipart("B").addFile("file", model, "a/b").toBytes(), StandardCharsets.UTF_8);
        assertTrue(text.contains("filename=\"castle %22v2%22.glb\""), text);
        assertEquals("a%0Db%0Ac", Multipart.escape("a\rb\nc"));
    }

    @Test
    void namesAreSentAsUtf8() throws IOException {
        Path model;
        try {
            model = Files.writeString(dir.resolve("château.glb"), "x");
        } catch (InvalidPathException e) {
            // A JVM whose file names are ASCII-only (POSIX locale) cannot even name the file.
            assumeTrue(false, "file names are not Unicode here: " + e.getMessage());
            return;
        }
        byte[] body = new Multipart("B").addFile("file", model, "a/b").toBytes();
        assertTrue(new String(body, StandardCharsets.UTF_8).contains("filename=\"château.glb\""));
    }

    @Test
    void publisherSendsTheSameBytesAsToBytes() throws Exception {
        Path model = Files.write(dir.resolve("m.glb"), new byte[] {1, 2, 3, 4, 5});
        Multipart body = new Multipart("XYZ").addFile("file", model, "application/octet-stream")
                .addText("settings", "application/json", "{}");
        try (FakeServer server = new FakeServer().on("/echo", exchange -> FakeServer.respond(exchange, 204, "text/plain", new byte[0]));
                HttpClient http = HttpClient.newBuilder().proxy(HttpClient.Builder.NO_PROXY).build()) {
            HttpRequest request = HttpRequest.newBuilder(server.uri().resolve("/echo"))
                    .header("Content-Type", body.contentType())
                    .POST(body.publisher())
                    .build();
            http.send(request, HttpResponse.BodyHandlers.discarding());
            assertArrayEquals(body.toBytes(), server.requests.get(0).body());
            // A known length, not chunked: the server sees the size up front.
            assertEquals(body.toBytes().length, body.publisher().contentLength());
        }
    }

    @Test
    void randomBoundariesDiffer() {
        assertNotEquals(Multipart.withRandomBoundary().contentType(), Multipart.withRandomBoundary().contentType());
    }
}
