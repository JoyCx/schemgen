package io.github.joycx.schemgen.common.backend;

import java.io.BufferedReader;
import java.io.EOFException;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.SecureRandom;
import java.time.Duration;
import java.util.ArrayDeque;
import java.util.Deque;
import java.util.HashMap;
import java.util.HexFormat;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import java.util.function.Consumer;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Starts schemgen2 as a sidecar of the game:
 *
 * <pre>schemgen2 serve --port 0 --exit-with-stdin --work-dir &lt;dir&gt; --job-ttl 24</pre>
 *
 * with a fresh random token in {@code SCHEMGEN_TOKEN} — the environment, not
 * the command line, which other users of the machine can read. The server
 * picks a free port and prints {@code listening http://127.0.0.1:<port>} on
 * stdout; its log goes to stderr, which is drained on a thread of its own so
 * the pipe never fills and stalls it. Standard input stays open: the server
 * exits when it closes, so it cannot outlive a game that crashes.
 */
public final class BackendLauncher {
    /** How long the server may take to print its address. */
    public static final Duration START_TIMEOUT = Duration.ofSeconds(20);
    /** How long it may then take to answer its health check. */
    static final Duration HEALTH_TIMEOUT = Duration.ofSeconds(10);
    private static final Pattern LISTENING = Pattern.compile("^listening (https?://\\S+)\\s*$");
    /** stderr lines kept to explain a server that failed to start. */
    private static final int TAIL_LINES = 12;

    /** Starts processes; tests replace it with fakes that script stdout. */
    @FunctionalInterface
    interface ProcessStarter {
        ServerProcess start(List<String> command, Map<String, String> environment) throws IOException;
    }

    /** The parts of a {@link Process} the launcher uses. */
    interface ServerProcess {
        OutputStream stdin();

        InputStream stdout();

        InputStream stderr();

        boolean isAlive();

        boolean waitFor(Duration timeout) throws InterruptedException;

        /** Exit code; only valid once the process ended. */
        int exitValue();

        void destroy();

        void destroyForcibly();
    }

    /** Real processes, inheriting the game's environment plus the given variables. */
    static final ProcessStarter SYSTEM = (command, environment) -> {
        ProcessBuilder builder = new ProcessBuilder(command);
        builder.environment().putAll(environment);
        Process process = builder.start();
        return new ServerProcess() {
            @Override
            public OutputStream stdin() {
                return process.getOutputStream();
            }

            @Override
            public InputStream stdout() {
                return process.getInputStream();
            }

            @Override
            public InputStream stderr() {
                return process.getErrorStream();
            }

            @Override
            public boolean isAlive() {
                return process.isAlive();
            }

            @Override
            public boolean waitFor(Duration timeout) throws InterruptedException {
                return process.waitFor(timeout.toMillis(), TimeUnit.MILLISECONDS);
            }

            @Override
            public int exitValue() {
                return process.exitValue();
            }

            @Override
            public void destroy() {
                process.destroy();
            }

            @Override
            public void destroyForcibly() {
                process.destroyForcibly();
            }
        };
    };

    private final ProcessStarter starter;
    private final Duration startTimeout;
    private final Duration healthTimeout;
    private final Consumer<String> log;

    /** A launcher of real processes; {@code log} receives the server's log lines. */
    public BackendLauncher(Consumer<String> log) {
        this(SYSTEM, START_TIMEOUT, HEALTH_TIMEOUT, log);
    }

    BackendLauncher(ProcessStarter starter, Duration startTimeout, Duration healthTimeout, Consumer<String> log) {
        this.starter = starter;
        this.startTimeout = startTimeout;
        this.healthTimeout = healthTimeout;
        this.log = log;
    }

    /**
     * Start {@code binary} and wait until it answers its health check.
     *
     * @param environment extra variables, e.g. {@code SCHEMGEN_PYTHON}
     * @throws BackendException "SchemGen server did not start: …" with the reason
     */
    public Sidecar launch(Path binary, Path workDir, Map<String, String> environment)
            throws IOException, InterruptedException {
        Files.createDirectories(workDir);
        String token = newToken();
        Map<String, String> env = new HashMap<>(environment);
        env.put("SCHEMGEN_TOKEN", token);
        List<String> command = List.of(binary.toString(), "serve", "--port", "0", "--exit-with-stdin",
                "--work-dir", workDir.toString(), "--job-ttl", "24");

        ServerProcess process;
        try {
            process = starter.start(command, env);
        } catch (IOException e) {
            throw new BackendException("SchemGen server did not start: " + e.getMessage(), e);
        }
        Tail stderr = new Tail(log);
        daemon("schemgen2-stderr", () -> stderr.drain(process.stderr()));
        CompletableFuture<URI> address = new CompletableFuture<>();
        daemon("schemgen2-stdout", () -> readAddress(process.stdout(), address));

        URI uri;
        try {
            uri = address.get(startTimeout.toMillis(), TimeUnit.MILLISECONDS);
        } catch (TimeoutException e) {
            stop(process);
            throw new BackendException("SchemGen server did not start: it printed no address within "
                    + seconds(startTimeout) + stderr.describe());
        } catch (ExecutionException e) {
            // stdout closed first: the process is exiting. Give stderr a moment to flush.
            process.waitFor(Duration.ofSeconds(2));
            stderr.awaitEnd(Duration.ofSeconds(1));
            String reason = exitDescription(process) + stderr.describe();
            stop(process);
            throw new BackendException("SchemGen server did not start: " + reason);
        } catch (InterruptedException e) {
            stop(process);
            throw e;
        }

        Sidecar sidecar = new Sidecar(process, uri, token);
        try {
            sidecar.awaitHealthy(healthTimeout);
        } catch (BackendException e) {
            sidecar.close();
            throw new BackendException("SchemGen server did not start: " + e.getMessage() + stderr.describe(), e);
        } catch (IOException | InterruptedException e) {
            sidecar.close();
            throw e;
        }
        log.accept("SchemGen server ready at " + uri);
        return sidecar;
    }

    /** The URL of a {@code listening <url>} line, if it is one. */
    static URI parseListening(String line) {
        Matcher m = LISTENING.matcher(line);
        return m.matches() ? URI.create(m.group(1)) : null;
    }

    /**
     * Stop a server: close its standard input (it shuts down gracefully,
     * cancelling what is running), then terminate it if it does not go.
     */
    static void stop(ServerProcess process) {
        try {
            process.stdin().close();
        } catch (IOException alreadyClosed) {
            // It may have exited already.
        }
        try {
            if (!process.waitFor(Duration.ofSeconds(3))) {
                process.destroy();
                if (!process.waitFor(Duration.ofSeconds(2))) {
                    process.destroyForcibly();
                }
            }
        } catch (InterruptedException e) {
            process.destroyForcibly();
            Thread.currentThread().interrupt();
        }
    }

    private void readAddress(InputStream stdout, CompletableFuture<URI> address) {
        try (BufferedReader reader = new BufferedReader(new InputStreamReader(stdout, StandardCharsets.UTF_8))) {
            for (String line; (line = reader.readLine()) != null; ) {
                URI uri = address.isDone() ? null : parseListening(line);
                if (uri != null) {
                    address.complete(uri);
                } else {
                    log.accept(line);
                }
            }
        } catch (IOException e) {
            // The pipe closed with the process.
        }
        address.completeExceptionally(new EOFException("stdout closed"));
    }

    /** "20 s", or "0.3 s" for the short timeouts of tests. */
    private static String seconds(Duration duration) {
        long millis = duration.toMillis();
        return (millis % 1000 == 0 ? Long.toString(millis / 1000) : Double.toString(millis / 1000.0)) + " s";
    }

    private static String exitDescription(ServerProcess process) {
        return process.isAlive() ? "it closed its output" : "it exited with code " + process.exitValue();
    }

    private static String newToken() {
        byte[] bytes = new byte[32];
        new SecureRandom().nextBytes(bytes);
        return HexFormat.of().formatHex(bytes);
    }

    private static void daemon(String name, Runnable task) {
        Thread thread = new Thread(task, name);
        thread.setDaemon(true);
        thread.start();
    }

    /** Drains stderr into the log and keeps the last lines to explain a failed start. */
    private static final class Tail {
        private final Consumer<String> log;
        private final Deque<String> lines = new ArrayDeque<>();
        private final CompletableFuture<Void> ended = new CompletableFuture<>();

        Tail(Consumer<String> log) {
            this.log = log;
        }

        void drain(InputStream stderr) {
            try (BufferedReader reader = new BufferedReader(new InputStreamReader(stderr, StandardCharsets.UTF_8))) {
                for (String line; (line = reader.readLine()) != null; ) {
                    log.accept(line);
                    synchronized (lines) {
                        lines.addLast(line);
                        if (lines.size() > TAIL_LINES) {
                            lines.removeFirst();
                        }
                    }
                }
            } catch (IOException e) {
                // The pipe closed with the process.
            } finally {
                ended.complete(null);
            }
        }

        void awaitEnd(Duration timeout) throws InterruptedException {
            try {
                ended.get(timeout.toMillis(), TimeUnit.MILLISECONDS);
            } catch (ExecutionException | TimeoutException e) {
                // Report whatever arrived.
            }
        }

        /** The most telling stderr line — the last one mentioning an error, else the last — as a suffix. */
        String describe() {
            synchronized (lines) {
                String chosen = null;
                for (String line : lines) {
                    if (line.toLowerCase(Locale.ROOT).contains("error")) {
                        chosen = line;
                    }
                }
                if (chosen == null) {
                    chosen = lines.peekLast();
                }
                return chosen == null ? "" : " (" + chosen.strip() + ")";
            }
        }
    }
}
