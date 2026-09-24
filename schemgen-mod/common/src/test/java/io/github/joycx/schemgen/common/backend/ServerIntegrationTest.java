package io.github.joycx.schemgen.common.backend;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.joycx.schemgen.common.config.ModConfig;
import io.github.joycx.schemgen.common.config.ServerMode;
import io.github.joycx.schemgen.common.model.Health;
import io.github.joycx.schemgen.common.model.Job;
import io.github.joycx.schemgen.common.preview.PreviewData;
import io.github.joycx.schemgen.common.preview.PreviewGrid;
import io.github.joycx.schemgen.common.schema.Schema;
import io.github.joycx.schemgen.common.schema.Targets;
import io.github.joycx.schemgen.common.settings.SettingsModel;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;
import java.util.List;
import java.util.Optional;
import java.util.concurrent.CopyOnWriteArrayList;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.condition.EnabledIfEnvironmentVariable;
import org.junit.jupiter.api.io.TempDir;

/**
 * The mod's backend layer against a real schemgen2, started the way the mod
 * starts it: sidecar mode, a fresh token, {@code --port 0}. Runs when
 * {@code SCHEMGEN_BINARY} names the binary; the conversions also need
 * {@code SCHEMGEN_PYTHON}, an interpreter with trimesh, until the server's
 * voxelizer no longer uses Python.
 */
@EnabledIfEnvironmentVariable(named = "SCHEMGEN_BINARY", matches = ".+")
class ServerIntegrationTest {
    private static final String PYTHON = System.getenv("SCHEMGEN_PYTHON");

    @TempDir
    static Path gameDir;

    private static final List<String> serverLog = new CopyOnWriteArrayList<>();
    private static BackendService service;
    private static BackendClient client;

    @BeforeAll
    static void startTheSidecar() throws Exception {
        ModConfig config = new ModConfig();
        config.serverMode = ServerMode.SIDECAR;
        config.binaryPath = System.getenv("SCHEMGEN_BINARY");
        config.pythonPath = PYTHON == null ? "" : PYTHON;
        ServerBinaries binaries = new ServerBinaries(null, Platform.current(), path -> null, (uri, target) -> {
            throw new AssertionError("the configured binary is used, nothing is downloaded");
        });
        service = new BackendService(() -> config, gameDir, binaries, new BackendLauncher(serverLog::add), serverLog::add);
        client = service.connect();
    }

    @AfterAll
    static void stopTheSidecar() {
        if (service != null) {
            service.close();
        }
    }

    private static Path fixture(String name) {
        Path path = Path.of(System.getProperty("schemgen.fixtures", "../../backend/fixtures")).resolve(name);
        assertTrue(Files.isRegularFile(path), "missing fixture " + path.toAbsolutePath());
        return path;
    }

    private static SettingsModel settings() throws Exception {
        Schema schema = client.schema();
        SettingsModel model = new SettingsModel(schema);
        model.setString("target", Targets.forGame(schema.targets(), "1.21.8", 4440));
        model.setNumber("max_size", 24);
        return model;
    }

    @Test
    void theSidecarIsHealthyAndRequiresItsToken() throws Exception {
        Health health = client.health();
        assertTrue(health.ok());
        assertEquals(2, health.api());
        assertTrue(health.auth(), "started with SCHEMGEN_TOKEN");
        assertEquals("127.0.0.1", client.baseUri().getHost());
        assertTrue(Files.isDirectory(gameDir.resolve("schemgen/work/outputs")), "--work-dir is the game's");

        BackendException refused = assertThrows(BackendException.class,
                () -> new BackendClient(client.baseUri(), null).schema());
        assertEquals(401, refused.status());
        assertTrue(refused.getMessage().startsWith("Missing or wrong token"), refused.getMessage());
        assertEquals(401, assertThrows(BackendException.class,
                () -> new BackendClient(client.baseUri(), "guess").jobs()).status());
    }

    @Test
    void theLiveSchemaDrivesTheSettingsModel() throws Exception {
        Schema schema = client.schema();
        assertEquals(client.health().version(), schema.version());
        SettingsModel model = new SettingsModel(schema);
        assertEquals(128, model.number("max_size"));
        assertEquals(schema.defaultTarget(), model.string("target"));
        assertFalse(model.toJson().has("output_dir"));
        assertNotNull(client.palette().get("minecraft:white_concrete"));
    }

    @Test
    @EnabledIfEnvironmentVariable(named = "SCHEMGEN_PYTHON", matches = ".+")
    void convertsFollowsDownloadsAndForgets() throws Exception {
        Path out = gameDir.resolve("schematics");
        List<Job> updates = new CopyOnWriteArrayList<>();
        List<String> started = new CopyOnWriteArrayList<>();
        Conversions.Listener listener = new Conversions.Listener() {
            @Override
            public void started(String jobId) {
                started.add(jobId);
            }

            @Override
            public void onUpdate(Job job) {
                updates.add(job);
            }
        };

        Conversions.Outcome outcome;
        try {
            outcome = Conversions.convert(client, fixture("textured.glb"), settings().toJson(), out, listener);
        } catch (BackendException e) {
            throw new AssertionError(e.getMessage() + "\nserver log:\n" + String.join("\n", serverLog), e);
        }

        Job done = outcome.job();
        assertEquals(Job.Status.DONE, done.status());
        assertEquals(done, updates.get(updates.size() - 1), "the stream's last event is the result");
        assertTrue(Arrays.stream(done.result().dims()).max().orElseThrow() <= 24, Arrays.toString(done.result().dims()));
        assertTrue(done.result().blocks() > 0);
        assertEquals("1.21.8", done.result().target());
        assertEquals(out.resolve("textured.litematic"), outcome.schematic());
        byte[] head = Arrays.copyOf(Files.readAllBytes(outcome.schematic()), 2);
        assertArrayEquals(new byte[] {0x1f, (byte) 0x8b}, head, "a gzip file, as .litematic files are");
        assertArrayEquals(new byte[] {(byte) 0x89, 'P', 'N', 'G'}, Arrays.copyOf(outcome.thumbnail(), 4));

        String id = started.get(0);
        assertTrue(client.jobs().stream().anyMatch(j -> j.id().equals(id)));
        assertEquals(Optional.empty(), client.cancel(id), "a finished job is forgotten");
        assertEquals(404, assertThrows(BackendException.class, () -> client.job(id)).status());
    }

    @Test
    @EnabledIfEnvironmentVariable(named = "SCHEMGEN_PYTHON", matches = ".+")
    void previewsComeBackAsBlocks() throws Exception {
        PreviewData preview = client.preview(fixture("textured.glb"), settings().toPreviewJson(16));

        PreviewGrid grid = preview.grid();
        assertTrue(grid.count() > 0);
        assertEquals(preview.count(), grid.count());
        assertTrue(Math.max(grid.sizeX(), Math.max(grid.sizeY(), grid.sizeZ())) <= 16);
        assertFalse(preview.palette().isEmpty());
        assertTrue(preview.palette().stream().allMatch(id -> id.startsWith("minecraft:")));
        assertEquals(grid.count(), preview.materials().stream().mapToInt(m -> m.count()).sum());
    }

    @Test
    @EnabledIfEnvironmentVariable(named = "SCHEMGEN_PYTHON", matches = ".+")
    void aRunningJobCanBeCancelled() throws Exception {
        // At 512 blocks the job runs for seconds; the cancel arrives within milliseconds.
        SettingsModel big = settings();
        big.setNumber("max_size", 512);
        String id = client.startJob(fixture("textured.glb"), big.toJson()).jobId();
        assertEquals(id, client.cancel(id).orElseThrow().id());
        Job last = client.events(id, job -> {});
        assertEquals(Job.Status.CANCELLED, last.status());
        assertEquals(409, assertThrows(BackendException.class, () -> client.download(id, gameDir.resolve("x"))).status());
    }
}
