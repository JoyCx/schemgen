package io.github.joycx.schemgen.common.backend;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.time.Duration;
import java.util.List;
import java.util.Map;
import java.util.concurrent.BlockingQueue;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.LinkedBlockingQueue;
import java.util.concurrent.TimeUnit;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class BackendLauncherTest {
    @TempDir
    Path dir;

    private final FakeServer health;
    private final List<String> log = new CopyOnWriteArrayList<>();

    BackendLauncherTest() throws IOException {
        health = new FakeServer().on("/api/health", ex -> FakeServer.json(ex, 200,
                "{\"status\":\"ok\",\"name\":\"schemgen2\",\"version\":\"2.0.0\",\"api\":2,\"auth\":true}"));
    }

    @AfterEach
    void stop() {
        health.close();
    }

    @Test
    void readsTheAddressAndPassesTheTokenOnlyThroughTheEnvironment() throws Exception {
        FakeProcess process = new FakeProcess();
        process.stdout.println("some banner");
        process.stdout.println("listening " + health.uri());
        process.stderr.println("[INFO] starting 4 workers");
        Recorder starter = new Recorder(process);

        Sidecar sidecar = launcher(starter, Duration.ofSeconds(5)).launch(Path.of("/opt/schemgen2"), dir,
                Map.of("SCHEMGEN_PYTHON", "/venv/bin/python"));

        assertEquals(health.uri(), sidecar.uri());
        assertEquals(List.of("/opt/schemgen2", "serve", "--port", "0", "--exit-with-stdin",
                "--work-dir", dir.toString(), "--job-ttl", "24"), starter.command);
        String token = starter.environment.get("SCHEMGEN_TOKEN");
        assertTrue(token.matches("[0-9a-f]{64}"), token);
        assertEquals(token, sidecar.token());
        assertFalse(String.join(" ", starter.command).contains(token), "never on the command line");
        assertEquals("/venv/bin/python", starter.environment.get("SCHEMGEN_PYTHON"));
        assertTrue(sidecar.isAlive());
        eventually(() -> log.contains("some banner") && log.contains("[INFO] starting 4 workers"));

        sidecar.close();
        assertTrue(process.stdinClosed, "stopped by closing its standard input");
        assertFalse(process.destroyed, "which was enough");
        assertFalse(sidecar.isAlive());
    }

    @Test
    void givesUpWhenNoAddressArrives() {
        FakeProcess process = new FakeProcess();
        process.stderr.println("[WARN] still thinking");

        BackendException e = assertThrows(BackendException.class,
                () -> launcher(new Recorder(process), Duration.ofMillis(300)).launch(Path.of("s"), dir, Map.of()));

        assertEquals("SchemGen server did not start: it printed no address within 0.3 s ([WARN] still thinking)",
                e.getMessage());
        assertTrue(process.stdinClosed && !process.isAlive(), "the process was stopped");
    }

    @Test
    void explainsAProcessThatExitsBeforeListening() {
        FakeProcess process = new FakeProcess();
        process.stderr.println("[INFO] loading palette");
        process.stderr.println("error: --work-dir: permission denied");
        process.stderr.println("Run `schemgen2 help serve` for the options.");
        process.exit(2);

        BackendException e = assertThrows(BackendException.class,
                () -> launcher(new Recorder(process), Duration.ofSeconds(5)).launch(Path.of("s"), dir, Map.of()));

        assertEquals("SchemGen server did not start: it exited with code 2 (error: --work-dir: permission denied)",
                e.getMessage());
    }

    @Test
    void reportsABinaryThatCannotRun() {
        BackendLauncher.ProcessStarter broken = (command, env) -> {
            throw new IOException("Cannot run program \"s\": error=13, Permission denied");
        };
        BackendException e = assertThrows(BackendException.class,
                () -> launcher(broken, Duration.ofSeconds(1)).launch(Path.of("s"), dir, Map.of()));
        assertEquals("SchemGen server did not start: Cannot run program \"s\": error=13, Permission denied",
                e.getMessage());
    }

    @Test
    void aServerThatNeverGetsHealthyIsStopped() throws IOException {
        try (FakeServer sick = new FakeServer()) {
            sick.on("/api/health", ex -> FakeServer.json(ex, 503, "{\"error\":\"warming up\"}"));
            FakeProcess process = new FakeProcess();
            process.stdout.println("listening " + sick.uri());

            BackendException e = assertThrows(BackendException.class,
                    () -> launcher(new Recorder(process), Duration.ofSeconds(5)).launch(Path.of("s"), dir, Map.of()));

            assertEquals("SchemGen server did not start: it did not answer its health check: warming up",
                    e.getMessage());
            assertTrue(process.stdinClosed);
        }
    }

    @Test
    void recognizesOnlyTheListeningLine() {
        assertEquals(URI.create("http://127.0.0.1:50731"),
                BackendLauncher.parseListening("listening http://127.0.0.1:50731"));
        assertEquals(URI.create("http://[::1]:3001"), BackendLauncher.parseListening("listening http://[::1]:3001  "));
        assertNull(BackendLauncher.parseListening("[INFO] SchemGen2 2.0.0 listening on http://127.0.0.1:1"));
        assertNull(BackendLauncher.parseListening("listening"));
    }

    private BackendLauncher launcher(BackendLauncher.ProcessStarter starter, Duration startTimeout) {
        return new BackendLauncher(starter, startTimeout, Duration.ofSeconds(1), log::add);
    }

    private static void eventually(java.util.function.BooleanSupplier condition) throws InterruptedException {
        for (int i = 0; i < 100 && !condition.getAsBoolean(); i++) {
            Thread.sleep(20);
        }
        assertTrue(condition.getAsBoolean());
    }

    /** Remembers how it was asked to start the process. */
    private static final class Recorder implements BackendLauncher.ProcessStarter {
        private final FakeProcess process;
        List<String> command;
        Map<String, String> environment;

        Recorder(FakeProcess process) {
            this.process = process;
        }

        @Override
        public BackendLauncher.ServerProcess start(List<String> command, Map<String, String> environment) {
            this.command = command;
            this.environment = environment;
            return process;
        }
    }

    /** A scripted process: tests write its output; closing its stdin ends it, like --exit-with-stdin. */
    private static final class FakeProcess implements BackendLauncher.ServerProcess {
        final Pipe stdout = new Pipe();
        final Pipe stderr = new Pipe();
        final CountDownLatch ended = new CountDownLatch(1);
        volatile boolean stdinClosed;
        volatile boolean destroyed;
        volatile int exitCode;

        synchronized void exit(int code) {
            if (ended.getCount() == 0) {
                return; // a process exits once
            }
            exitCode = code;
            stdout.close();
            stderr.close();
            ended.countDown();
        }

        @Override
        public OutputStream stdin() {
            return new OutputStream() {
                @Override
                public void write(int b) {}

                @Override
                public void close() {
                    stdinClosed = true;
                    exit(0);
                }
            };
        }

        @Override
        public InputStream stdout() {
            return stdout;
        }

        @Override
        public InputStream stderr() {
            return stderr;
        }

        @Override
        public boolean isAlive() {
            return ended.getCount() > 0;
        }

        @Override
        public boolean waitFor(Duration timeout) throws InterruptedException {
            return ended.await(timeout.toMillis(), TimeUnit.MILLISECONDS);
        }

        @Override
        public int exitValue() {
            return exitCode;
        }

        @Override
        public void destroy() {
            destroyed = true;
            exit(143);
        }

        @Override
        public void destroyForcibly() {
            destroy();
        }
    }

    /** An input stream fed line by line, ending only when closed. */
    private static final class Pipe extends InputStream {
        private static final int EOF = -1;
        private final BlockingQueue<Integer> bytes = new LinkedBlockingQueue<>();

        void println(String line) {
            for (byte b : (line + "\n").getBytes(StandardCharsets.UTF_8)) {
                bytes.add(b & 0xff);
            }
        }

        @Override
        public void close() {
            bytes.add(EOF);
        }

        @Override
        public int read() throws IOException {
            try {
                int b = bytes.take();
                if (b == EOF) {
                    bytes.add(EOF); // stay at the end for later reads
                }
                return b;
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                throw new IOException(e);
            }
        }

        /** Like a pipe: wait for the first byte, then return what is there instead of filling the buffer. */
        @Override
        public int read(byte[] buffer, int offset, int length) throws IOException {
            if (length == 0) {
                return 0;
            }
            int first = read();
            if (first == EOF) {
                return -1;
            }
            buffer[offset] = (byte) first;
            int n = 1;
            for (Integer next; n < length && (next = bytes.peek()) != null && next != EOF; n++) {
                buffer[offset + n] = (byte) (int) bytes.poll();
            }
            return n;
        }
    }
}
