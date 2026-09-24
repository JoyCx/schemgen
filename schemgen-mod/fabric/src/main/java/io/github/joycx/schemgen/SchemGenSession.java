package io.github.joycx.schemgen;

import io.github.joycx.schemgen.common.backend.BackendLauncher;
import io.github.joycx.schemgen.common.backend.BackendService;
import io.github.joycx.schemgen.common.backend.Platform;
import io.github.joycx.schemgen.common.backend.ServerBinaries;
import io.github.joycx.schemgen.common.config.ModConfig;
import io.github.joycx.schemgen.common.schema.Schema;
import io.github.joycx.schemgen.common.schema.Targets;
import io.github.joycx.schemgen.common.settings.SettingsModel;
import io.github.joycx.schemgen.litematica.LitematicaSupport;
import io.github.joycx.schemgen.preview.GhostPreview;
import io.github.joycx.schemgen.ui.FilePicker;
import java.io.IOException;
import java.nio.file.Path;
import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.function.Consumer;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.SharedConstants;
import net.minecraft.client.MinecraftClient;
import net.minecraft.text.Text;
import net.minecraft.util.math.BlockPos;

/**
 * One game session of the mod: the config, the server connection, the
 * settings form's model, the model list, and the conversion and preview in
 * progress.
 *
 * <p>All of it belongs to the client thread. Whatever blocks — starting the
 * server, every HTTP call — runs on a worker thread and hands its result back
 * with {@link #onClient}, so the game never waits on the server.
 */
public final class SchemGenSession {
    /** Where the connection to the server stands. */
    public enum Connection { IDLE, CONNECTING, READY, FAILED }

    /** Blocking work for the worker thread. */
    @FunctionalInterface
    public interface Task {
        void run() throws IOException, InterruptedException;
    }

    private final Path gameDir;
    private final Path configFile;
    private final ExecutorService worker = Executors.newCachedThreadPool(task -> {
        Thread thread = new Thread(task, "SchemGen worker");
        thread.setDaemon(true);
        return thread;
    });
    private final BackendService backend;
    private final Converter converter = new Converter(this);
    private final GhostPreview preview = new GhostPreview(this);

    /** Replaced, never changed in place: the worker reads it while connecting. */
    private volatile ModConfig config;
    private Connection connection = Connection.IDLE;
    private String connectionError = "";
    private SettingsModel settings;
    /** The connected server's version, from its schema. */
    private String serverVersion = "";
    private List<Path> models = List.of();
    private Path selectedModel;
    private boolean showAdvanced;
    /** Bumped whenever an open screen has to rebuild its widgets. */
    private int revision;

    private SchemGenSession(Path gameDir, Path configFile, ModConfig config) {
        this.gameDir = gameDir;
        this.configFile = configFile;
        this.config = config;
        this.backend = new BackendService(() -> this.config, gameDir, binaries(),
                new BackendLauncher(line -> SchemGenClient.LOGGER.info("[schemgen2] {}", line)),
                SchemGenClient.LOGGER::info);
        refreshModels();
    }

    static SchemGenSession start() {
        FabricLoader loader = FabricLoader.getInstance();
        Path configFile = loader.getConfigDir().resolve(ModConfig.FILE_NAME);
        return new SchemGenSession(loader.getGameDir(), configFile, ModConfig.load(configFile, SchemGenClient.LOGGER::warn));
    }

    /** The server binaries bundled in (or pinned by) this jar. */
    private static ServerBinaries binaries() {
        try {
            return ServerBinaries.forCurrentPlatform(SchemGenClient.class::getResourceAsStream);
        } catch (IOException e) {
            SchemGenClient.LOGGER.warn("Cannot read the pinned server release; only external servers will work", e);
            return new ServerBinaries(null, Platform.current(), path -> null, (uri, target) -> {});
        }
    }

    // ── Connection ──────────────────────────────────────────────────────────

    /** Connect in the background — starting the sidecar if needed — unless connected or connecting. */
    public void connect() {
        if (connection == Connection.CONNECTING || connection == Connection.READY) {
            return;
        }
        connection = Connection.CONNECTING;
        connectionError = "";
        revision++;
        run(() -> {
            Schema schema = backend.connect().schema();
            onClient(() -> connected(schema));
        }, error -> {
            connection = Connection.FAILED;
            connectionError = error;
            revision++;
        });
    }

    private void connected(Schema schema) {
        SettingsModel model = new SettingsModel(schema);
        model.restore(config.lastSettings);
        if (schema.field("target").isPresent()) {
            model.setString("target", target(schema));
        }
        settings = model;
        serverVersion = schema.version() == null ? "" : schema.version();
        connection = Connection.READY;
        revision++;
    }

    /** The config's target override, else the running game's own version. */
    private String target(Schema schema) {
        if (!config.targetOverride.isBlank()) {
            return config.targetOverride;
        }
        String game = FabricLoader.getInstance().getModContainer("minecraft")
                .map(minecraft -> minecraft.getMetadata().getVersion().getFriendlyString())
                .orElse("");
        return Targets.forGame(schema.targets(), game, SharedConstants.WORLD_VERSION);
    }

    /** Convert for another Minecraft version than the game's, or back to the game's with a blank one. */
    public void setTargetOverride(String target) {
        ModConfig next = config.sanitized();
        next.targetOverride = target;
        updateConfig(next, false);
        if (settings != null && settings.schema().field("target").isPresent()) {
            settings.setString("target", target(settings.schema()));
        }
        revision++;
    }

    /**
     * Publish and save a changed config. With {@code serverChanged}, the old
     * server is let go (a sidecar is stopped) and the connection starts over.
     */
    public void updateConfig(ModConfig next, boolean serverChanged) {
        config = next.sanitized();
        try {
            config.save(configFile);
        } catch (IOException e) {
            SchemGenClient.LOGGER.warn("Cannot save {}", configFile, e);
        }
        if (serverChanged) {
            saveSettings();
            settings = null;
            connection = Connection.CONNECTING;
            connectionError = "";
            revision++;
            run(() -> {
                backend.reset();
                Schema schema = backend.connect().schema();
                onClient(() -> connected(schema));
            }, error -> {
                connection = Connection.FAILED;
                connectionError = error;
                revision++;
            });
        }
        refreshModels();
    }

    /** Remember the form's settings for the next session. */
    public void saveSettings() {
        if (settings == null) {
            return;
        }
        ModConfig next = config.sanitized();
        next.lastSettings = settings.snapshot();
        config = next;
        try {
            config.save(configFile);
        } catch (IOException e) {
            SchemGenClient.LOGGER.warn("Cannot save {}", configFile, e);
        }
    }

    // ── Models ──────────────────────────────────────────────────────────────

    /** List the models folder again. */
    public void refreshModels() {
        Path folder = modelsFolder();
        try {
            models = FilePicker.list(folder);
        } catch (IOException e) {
            SchemGenClient.LOGGER.warn("Cannot list {}", folder, e);
            models = List.of();
        }
        revision++;
    }

    public void select(Path model) {
        selectedModel = model;
        revision++;
    }

    // ── Litematica ──────────────────────────────────────────────────────────

    /**
     * Place a finished schematic in Litematica: where the preview is and
     * turned the same way when there is one, else at the player's feet.
     *
     * @return what happened, for the status line
     */
    public Text placeInLitematica(Path schematic, int[] dims) {
        MinecraftClient client = MinecraftClient.getInstance();
        if (!LitematicaSupport.isLoaded()) {
            return Text.translatable("schemgen.litematica.missing");
        }
        if (client.player == null) {
            return Text.translatable("schemgen.litematica.no_world");
        }
        GhostPreview.Placement placement = preview.placementFor(dims);
        BlockPos origin = placement == null ? client.player.getBlockPos() : placement.origin();
        int turns = placement == null ? 0 : placement.quarterTurns();
        preview.clear(); // the real schematic takes the preview's place
        Text error = LitematicaSupport.place(schematic, origin, turns);
        return error != null ? error
                : Text.translatable("schemgen.litematica.placed", schematic.getFileName().toString());
    }

    // ── Threads ─────────────────────────────────────────────────────────────

    /**
     * Run {@code task} on the worker. A failure reaches {@code onError} on the
     * client thread as the sentence to show the player.
     */
    public void run(Task task, Consumer<String> onError) {
        worker.execute(() -> {
            try {
                task.run();
            } catch (IOException e) {
                SchemGenClient.LOGGER.warn("{}", e.getMessage());
                onClient(() -> onError.accept(e.getMessage()));
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
            } catch (RuntimeException e) {
                SchemGenClient.LOGGER.error("SchemGen task failed", e);
                onClient(() -> onError.accept(e.toString()));
            }
        });
    }

    /** Run {@code action} on the client thread. */
    public static void onClient(Runnable action) {
        MinecraftClient.getInstance().execute(action);
    }

    /** At game exit: remember the settings, drop the preview, stop the server. */
    void shutdown() {
        saveSettings();
        preview.clear();
        worker.shutdownNow();
        backend.close();
    }

    // ── State for the screens ───────────────────────────────────────────────

    public ModConfig config() {
        return config;
    }

    public BackendService backend() {
        return backend;
    }

    public Connection connection() {
        return connection;
    }

    public String connectionError() {
        return connectionError;
    }

    public String serverVersion() {
        return serverVersion;
    }

    /** The settings form's model; {@code null} until the server's schema arrived. */
    public SettingsModel settings() {
        return settings;
    }

    public List<Path> models() {
        return models;
    }

    public Path selectedModel() {
        return selectedModel;
    }

    public boolean showAdvanced() {
        return showAdvanced;
    }

    public void setShowAdvanced(boolean show) {
        showAdvanced = show;
        revision++;
    }

    /** Ask open screens to rebuild their widgets, e.g. after a setting other fields depend on changed. */
    public void changed() {
        revision++;
    }

    public int revision() {
        return revision;
    }

    public Converter converter() {
        return converter;
    }

    public GhostPreview preview() {
        return preview;
    }

    public Path modelsFolder() {
        return config.modelsFolder(gameDir);
    }

    public Path outputFolder() {
        return config.outputFolder(gameDir);
    }

    /** Where Litematica previews are converted to; emptied as previews are cleared. */
    public Path previewFolder() {
        return gameDir.resolve("schemgen").resolve("preview");
    }
}
