package io.github.joycx.schemgen.common.backend;

import io.github.joycx.schemgen.common.config.ModConfig;
import io.github.joycx.schemgen.common.config.ServerMode;
import io.github.joycx.schemgen.common.model.Health;
import java.io.IOException;
import java.nio.file.Path;
import java.util.Objects;
import java.util.function.Consumer;
import java.util.function.Supplier;

/**
 * The mod's one way to a server: in sidecar mode it starts schemgen2 the
 * first time it is needed and keeps it for the session; in external mode it
 * connects to the configured address. Either way, {@link #connect()} returns
 * a client of a server that just answered its health check.
 */
public final class BackendService implements AutoCloseable {
    private final Supplier<ModConfig> config;
    private final Path gameDir;
    private final ServerBinaries binaries;
    private final BackendLauncher launcher;
    private final Consumer<String> log;

    private Sidecar sidecar;
    private BackendClient external;
    /** The address and token {@link #external} was made for. */
    private String externalKey;

    public BackendService(Supplier<ModConfig> config, Path gameDir, ServerBinaries binaries,
            BackendLauncher launcher, Consumer<String> log) {
        this.config = config;
        this.gameDir = gameDir;
        this.binaries = binaries;
        this.launcher = launcher;
        this.log = log;
    }

    /**
     * A ready client, starting the sidecar first when needed. Blocks — for up
     * to half a minute on a first start — so call it off the render thread.
     */
    public synchronized BackendClient connect() throws IOException, InterruptedException {
        ModConfig c = config.get();
        return c.serverMode == ServerMode.EXTERNAL ? connectExternal(c) : connectSidecar(c);
    }

    /** Forget the connection, stopping a sidecar — after the server settings changed. */
    public synchronized void reset() {
        if (sidecar != null) {
            sidecar.close();
            sidecar = null;
        }
        if (external != null) {
            external.close();
            external = null;
        }
    }

    @Override
    public void close() {
        reset();
    }

    private BackendClient connectExternal(ModConfig c) throws IOException, InterruptedException {
        if (sidecar != null) {
            sidecar.close();
            sidecar = null;
        }
        String key = c.externalUri() + " " + c.token;
        if (external == null || !key.equals(externalKey)) {
            if (external != null) {
                external.close();
            }
            external = new BackendClient(c.externalUri(), c.token);
            externalKey = key;
        }
        check(external.health(), c.externalUri().toString());
        return external;
    }

    private BackendClient connectSidecar(ModConfig c) throws IOException, InterruptedException {
        if (external != null) {
            external.close();
            external = null;
        }
        if (sidecar != null && sidecar.isAlive()) {
            return sidecar.client();
        }
        if (sidecar != null) {
            log.accept("The SchemGen server stopped unexpectedly; starting it again");
            sidecar.close();
            sidecar = null;
        }
        ServerBinaries.Resolution resolution = binaries.resolve(c.binaryPath, gameDir);
        if (resolution instanceof ServerBinaries.Unavailable unavailable) {
            throw new BackendException("SchemGen server did not start: " + unavailable.reason());
        }
        ServerBinaries.Found found = (ServerBinaries.Found) resolution;
        log.accept("Starting the SchemGen server (" + found.origin() + "): " + found.binary());
        sidecar = launcher.launch(found.binary(), gameDir.resolve("schemgen").resolve("work"));
        check(sidecar.client().health(), sidecar.uri().toString());
        return sidecar.client();
    }

    /** API v2 is what the mod speaks; a server from before it has no job routes. */
    private static void check(Health health, String where) throws BackendException {
        if (!health.ok()) {
            throw new BackendException("The SchemGen server at " + where + " is not ok: " + health.status());
        }
        if (health.api() < 2) {
            throw new BackendException("The server at " + where + " is schemgen2 "
                    + Objects.requireNonNullElse(health.version(), "(unknown version)")
                    + ", which is too old for the mod: it needs API v2 (schemgen2 2.1 or later)");
        }
    }
}
