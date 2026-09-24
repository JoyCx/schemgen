package io.github.joycx.schemgen.common.backend;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.JsonObject;
import io.github.joycx.schemgen.common.Fixtures;
import io.github.joycx.schemgen.common.model.Health;
import io.github.joycx.schemgen.common.model.Job;
import io.github.joycx.schemgen.common.model.StartedJobs;
import io.github.joycx.schemgen.common.preview.PreviewData;
import java.io.IOException;
import java.io.OutputStream;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Base64;
import java.util.List;
import java.util.Map;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.stream.Stream;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class BackendClientTest {
    private final FakeServer server;
    @TempDir
    Path dir;

    BackendClientTest() throws IOException {
        server = new FakeServer();
    }

    @AfterEach
    void stop() {
        server.close();
    }

    private static String job(String status, double progress) {
        return "{\"id\":\"j1\",\"status\":\"" + status + "\",\"progress\":" + progress
                + ",\"stage\":\"x\",\"message\":\"m\",\"download_name\":\"castle.litematic\",\"error\":"
                + ("error".equals(status) ? "\"Model has no triangles\"" : "null") + "}";
    }

    @Test
    void sendsTheBearerTokenOnlyWhenThereIsOne() throws Exception {
        server.on("/api/health", ex -> FakeServer.json(ex, 200,
                "{\"status\":\"ok\",\"name\":\"schemgen2\",\"version\":\"2.0.0\",\"api\":2,\"auth\":true}"));
        assertTrue(new BackendClient(server.uri(), "s3cret").health().ok());
        assertTrue(new BackendClient(server.uri(), "  ").health().auth());
        assertEquals("Bearer s3cret", server.requests.get(0).authorization());
        assertNull(server.requests.get(1).authorization());
    }

    @Test
    void readsTheRealServersHealthAnswer() throws Exception {
        server.on("/api/health", ex -> FakeServer.json(ex, 200, Fixtures.text("health.json")));
        Health health = new BackendClient(server.uri(), null).health();
        assertTrue(health.ok());
        assertEquals(2, health.api());
        assertEquals("1.21.8", health.target());
        assertEquals(4440, health.dataVersion());
        assertEquals(7, health.schematicVersion());
        assertTrue(health.auth());
    }

    @Test
    void apiErrorsCarryTheServersMessage() {
        server.on("/api/jobs/", ex -> FakeServer.json(ex, 404, "{\"error\":\"Unknown job\"}"));
        server.on("/api/schema", ex -> FakeServer.respond(ex, 502, "text/plain", "Bad gateway".getBytes(StandardCharsets.UTF_8)));
        BackendClient client = new BackendClient(server.uri(), null);

        BackendException unknown = assertThrows(BackendException.class, () -> client.job("nope"));
        assertEquals("Unknown job", unknown.getMessage());
        assertEquals(404, unknown.status());
        assertEquals("HTTP 502: Bad gateway", assertThrows(BackendException.class, client::schema).getMessage());
        assertThrows(IllegalArgumentException.class, () -> client.job("../../etc"));
    }

    @Test
    void anUnreachableServerIsReportedAsSuch() throws IOException {
        URI closed;
        try (FakeServer gone = new FakeServer()) {
            closed = gone.uri();
        }
        BackendException e = assertThrows(BackendException.class, () -> new BackendClient(closed, null).health());
        assertTrue(e.getMessage().startsWith("Cannot reach the SchemGen server at " + closed), e.getMessage());
        assertEquals(0, e.status());
    }

    @Test
    void startJobUploadsTheModelAndTheSettings() throws Exception {
        server.on("/api/jobs", ex -> FakeServer.json(ex, 202,
                "{\"job_id\":\"j1\",\"jobs\":[{\"job_id\":\"j1\",\"filename\":\"castle.glb\",\"name\":\"castle\"}],"
                        + "\"skipped\":[],\"ignored\":[\"max_sise\"],\"output_dir\":null}"));
        Path model = Files.write(dir.resolve("castle.glb"), new byte[] {9, 8, 7});
        JsonObject settings = new JsonObject();
        settings.addProperty("max_sise", 64);
        settings.add("voxel_size", com.google.gson.JsonNull.INSTANCE);

        StartedJobs started = new BackendClient(server.uri(), "t").startJob(model, settings);

        assertEquals("j1", started.jobId());
        assertEquals(List.of("max_sise"), started.ignored());
        FakeServer.Request request = server.requests.get(0);
        assertEquals("POST", request.method());
        assertTrue(request.contentType().startsWith("multipart/form-data; boundary="));
        String body = new String(request.body(), StandardCharsets.ISO_8859_1);
        assertTrue(body.contains("name=\"file\"; filename=\"castle.glb\""));
        assertTrue(body.contains("\r\n\r\n\u0009\u0008\u0007\r\n"), "file bytes verbatim");
        assertTrue(body.contains("{\"max_sise\":64,\"voxel_size\":null}"), "nulls are sent: " + body);
    }

    @Test
    void eventsFollowTheStreamToItsLastEvent() throws Exception {
        server.on("/api/jobs/j1/events", ex -> {
            ex.getResponseHeaders().set("Content-Type", "text/event-stream");
            ex.sendResponseHeaders(200, 0);
            try (OutputStream out = ex.getResponseBody()) {
                String stream = "retry: 3000\n\nevent: progress\ndata: " + job("running", 40) + "\n\n"
                        + ": keep-alive\n\nevent: done\ndata: " + job("done", 100) + "\n\n";
                for (byte b : stream.getBytes(StandardCharsets.UTF_8)) {
                    out.write(b); // byte by byte, to split every line and event
                    out.flush();
                }
            }
        });
        List<Job> seen = new ArrayList<>();
        Job last = new BackendClient(server.uri(), null).events("j1", seen::add);

        assertEquals(Job.Status.DONE, last.status());
        assertEquals(List.of(Job.Status.RUNNING, Job.Status.DONE), seen.stream().map(Job::status).toList());
        assertEquals(1, server.requests.size(), "no polling needed");
    }

    @Test
    void eventsFallBackToPollingWhenTheStreamFails() throws Exception {
        AtomicInteger polls = new AtomicInteger();
        server.on("/api/jobs/j1/events", ex -> FakeServer.respond(ex, 503, "text/plain", new byte[0]));
        server.on("/api/jobs/j1", ex -> FakeServer.json(ex, 200,
                polls.incrementAndGet() < 3 ? job("running", 10 * polls.get()) : job("done", 100)));
        List<Job> seen = new ArrayList<>();

        Job last = new BackendClient(server.uri(), null).events("j1", seen::add);

        assertEquals(Job.Status.DONE, last.status());
        assertEquals(3, polls.get());
        assertEquals(3, seen.size());
    }

    @Test
    void eventsPollWhenTheStreamEndsEarly() throws Exception {
        server.on("/api/jobs/j1/events", ex -> {
            byte[] body = ("event: progress\ndata: " + job("running", 5) + "\n\n").getBytes(StandardCharsets.UTF_8);
            FakeServer.respond(ex, 200, "text/event-stream", body);
        });
        server.on("/api/jobs/j1", ex -> FakeServer.json(ex, 200, job("error", 50)));

        Job last = new BackendClient(server.uri(), null).events("j1", job -> {});

        assertEquals(Job.Status.ERROR, last.status());
        assertEquals("Model has no triangles", last.error());
    }

    @Test
    void eventsForAnUnknownJobFailWithoutPolling() {
        server.on("/api/jobs/", ex -> FakeServer.json(ex, 404, "{\"error\":\"Unknown job\"}"));
        BackendException e = assertThrows(BackendException.class,
                () -> new BackendClient(server.uri(), null).events("zzz", job -> {}));
        assertEquals(404, e.status());
        assertEquals(1, server.requests.size());
    }

    @Test
    void downloadWritesTheWholeFileOrNothing() throws Exception {
        byte[] schematic = {0x1f, (byte) 0x8b, 8, 0, 1, 2, 3};
        server.on("/api/jobs/j1/download", ex -> FakeServer.respond(ex, 200, "application/octet-stream", schematic));
        server.on("/api/jobs/j2/download", ex -> FakeServer.json(ex, 409, "{\"error\":\"Conversion is not finished yet\"}"));
        BackendClient client = new BackendClient(server.uri(), null);

        Path target = dir.resolve("out").resolve("castle.litematic");
        client.download("j1", target);
        assertArrayEquals(schematic, Files.readAllBytes(target));

        Path other = dir.resolve("out").resolve("other.litematic");
        assertEquals("Conversion is not finished yet",
                assertThrows(BackendException.class, () -> client.download("j2", other)).getMessage());
        assertFalse(Files.exists(other));
        try (Stream<Path> files = Files.list(dir.resolve("out"))) {
            assertEquals(List.of(target), files.toList(), "no temporary files left behind");
        }
    }

    @Test
    void previewDecodesThePackedBlocks() throws Exception {
        String blocks = Base64.getEncoder().encodeToString(new byte[] {
            1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 0, 0, 0, 0, 0, 0});
        server.on("/api/preview", ex -> FakeServer.json(ex, 200, "{\"dims\":[4,4,600],\"origin\":[0,0,0],\"pitch\":0.5,"
                + "\"palette\":[\"minecraft:dirt\",\"minecraft:stone\"],\"count\":2,\"blocks\":\"" + blocks + "\","
                + "\"materials\":[],\"target\":\"1.21.8\",\"capped\":false,\"seconds\":0.1}"));
        Path model = Files.writeString(dir.resolve("m.glb"), "x");

        PreviewData preview = new BackendClient(server.uri(), null).preview(model, new JsonObject());

        assertArrayEquals(new int[] {1, 2, 3, 1, 0, 0, 258, 0}, preview.decodeBlocks());
    }

    @Test
    void cancelReturnsTheJobOrNothingForAForgottenOne() throws Exception {
        server.on("/api/jobs/j1", ex -> FakeServer.json(ex, 202, job("running", 20)));
        server.on("/api/jobs/j2", ex -> FakeServer.respond(ex, 204, "application/json", new byte[0]));
        BackendClient client = new BackendClient(server.uri(), null);

        assertEquals("j1", client.cancel("j1").orElseThrow().id());
        assertTrue(client.cancel("j2").isEmpty());
        assertEquals("DELETE", server.requests.get(0).method());
    }

    @Test
    void paletteColorsBecomeRgbByFullId() throws Exception {
        server.on("/api/palette", ex -> FakeServer.json(ex, 200,
                "{\"stone\":[125.0,125.0,125.0],\"minecraft:red_wool\":[160.4,39.0,34.0]}"));
        Map<String, Integer> palette = new BackendClient(server.uri(), null).palette();
        assertEquals(0x7d7d7d, palette.get("minecraft:stone"));
        assertEquals(0xa02722, palette.get("minecraft:red_wool"));
    }
}
