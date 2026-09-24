package io.github.joycx.schemgen.common.backend;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.JsonObject;
import io.github.joycx.schemgen.common.Json;
import io.github.joycx.schemgen.common.model.Job;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class ConversionsTest {
    @TempDir
    Path dir;

    private final FakeServer server;
    private final List<String> started = new ArrayList<>();
    private final List<Job> updates = new ArrayList<>();
    private final Conversions.Listener listener = new Conversions.Listener() {
        @Override
        public void started(String jobId) {
            started.add(jobId);
        }

        @Override
        public void onUpdate(Job job) {
            updates.add(job);
        }
    };

    ConversionsTest() throws IOException {
        server = new FakeServer();
    }

    @AfterEach
    void stop() {
        server.close();
    }

    /** A job whose event stream reports {@code finalJson} as its last event, named after its status. */
    private void jobEndsAs(String finalJson) {
        String event = Json.API.fromJson(finalJson, Job.class).status().name().toLowerCase(java.util.Locale.ROOT);
        server.on("/api/jobs", ex -> FakeServer.json(ex, 202, "{\"job_id\":\"j1\",\"jobs\":[],\"skipped\":[],\"ignored\":[]}"));
        server.on("/api/jobs/j1/events", ex -> FakeServer.respond(ex, 200, "text/event-stream",
                ("event: " + event + "\ndata: " + finalJson + "\n\n").getBytes(StandardCharsets.UTF_8)));
    }

    @Test
    void aFinishedJobLandsNextToExistingSchematicsWithoutReplacingThem() throws Exception {
        jobEndsAs("{\"id\":\"j1\",\"status\":\"done\",\"download_name\":\"castle.litematic\"}");
        server.on("/api/jobs/j1/download", ex -> FakeServer.respond(ex, 200, "application/octet-stream", new byte[] {1, 2}));
        server.on("/api/jobs/j1/thumbnail.png", ex -> FakeServer.respond(ex, 200, "image/png", new byte[] {(byte) 0x89, 'P'}));
        Files.writeString(dir.resolve("castle.litematic"), "the player's own build");
        Path model = Files.writeString(dir.resolve("castle.glb"), "glTF");

        Conversions.Outcome outcome = Conversions.convert(new BackendClient(server.uri(), null), model, new JsonObject(), dir, listener);

        assertEquals(dir.resolve("castle-2.litematic"), outcome.schematic());
        assertArrayEquals(new byte[] {1, 2}, Files.readAllBytes(outcome.schematic()));
        assertEquals("the player's own build", Files.readString(dir.resolve("castle.litematic")));
        assertArrayEquals(new byte[] {(byte) 0x89, 'P'}, outcome.thumbnail());
        assertEquals(List.of("j1"), started);
        assertEquals(1, updates.size());
    }

    @Test
    void aFailedJobThrowsTheServersReason() {
        jobEndsAs("{\"id\":\"j1\",\"status\":\"error\",\"error\":\"The model has no triangles\"}");
        BackendException e = assertThrows(BackendException.class, () -> Conversions.convert(
                new BackendClient(server.uri(), null), Files.writeString(dir.resolve("m.glb"), "x"), new JsonObject(), dir, listener));
        assertEquals("The model has no triangles", e.getMessage());
    }

    @Test
    void aCancelledJobHasNoFile() throws Exception {
        jobEndsAs("{\"id\":\"j1\",\"status\":\"cancelled\"}");
        Conversions.Outcome outcome = Conversions.convert(new BackendClient(server.uri(), null),
                Files.writeString(dir.resolve("m.glb"), "x"), new JsonObject(), dir, listener);
        assertTrue(outcome.cancelled());
        assertNull(outcome.schematic());
    }

    @Test
    void serverFileNamesAreMadeSafe() {
        assertEquals("castle.litematic", Conversions.fileName(job("castle.litematic")));
        assertEquals("a_b_c.litematic", Conversions.fileName(job("a/b:c.litematic")));
        assertEquals("x.litematic", Conversions.fileName(job("x")));
        assertEquals("schematic.litematic", Conversions.fileName(job(null)));
        assertEquals("schematic.litematic", Conversions.fileName(job("..")));
    }

    private static Job job(String downloadName) {
        JsonObject json = new JsonObject();
        json.addProperty("id", "j");
        json.addProperty("download_name", downloadName);
        return Json.API.fromJson(json, Job.class);
    }
}
