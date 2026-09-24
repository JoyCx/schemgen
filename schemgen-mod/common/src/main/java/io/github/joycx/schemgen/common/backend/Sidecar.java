package io.github.joycx.schemgen.common.backend;

import io.github.joycx.schemgen.common.model.Health;
import java.io.IOException;
import java.net.URI;
import java.time.Duration;

/** A schemgen2 the mod started, and the client that talks to it. */
public final class Sidecar implements AutoCloseable {
    private final BackendLauncher.ServerProcess process;
    private final URI uri;
    private final String token;
    private final BackendClient client;
    private boolean closed;

    Sidecar(BackendLauncher.ServerProcess process, URI uri, String token) {
        this.process = process;
        this.uri = uri;
        this.token = token;
        this.client = new BackendClient(uri, token);
    }

    public URI uri() {
        return uri;
    }

    public String token() {
        return token;
    }

    public BackendClient client() {
        return client;
    }

    public boolean isAlive() {
        return process.isAlive();
    }

    /** Poll {@code /api/health} until the server says it is ok. */
    void awaitHealthy(Duration timeout) throws IOException, InterruptedException {
        long deadline = System.nanoTime() + timeout.toNanos();
        BackendException last = null;
        while (System.nanoTime() < deadline) {
            if (!process.isAlive()) {
                throw new BackendException("it exited with code " + process.exitValue());
            }
            try {
                Health health = client.health();
                if (health.ok()) {
                    return;
                }
                last = new BackendException("its health check says " + health.status());
            } catch (BackendException e) {
                last = e;
            }
            Thread.sleep(100);
        }
        throw new BackendException("it did not answer its health check"
                + (last == null ? "" : ": " + last.getMessage()));
    }

    /** Stop the server and abort requests still in flight. Safe to call twice. */
    @Override
    public synchronized void close() {
        if (closed) {
            return;
        }
        closed = true;
        client.close();
        BackendLauncher.stop(process);
    }
}
