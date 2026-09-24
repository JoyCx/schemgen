package io.github.joycx.schemgen.common.backend;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParseException;
import io.github.joycx.schemgen.common.Json;
import io.github.joycx.schemgen.common.files.AtomicFiles;
import io.github.joycx.schemgen.common.model.Health;
import io.github.joycx.schemgen.common.model.Job;
import io.github.joycx.schemgen.common.model.StartedJobs;
import io.github.joycx.schemgen.common.preview.PreviewData;
import io.github.joycx.schemgen.common.schema.Schema;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.Reader;
import java.net.ConnectException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.net.http.HttpTimeoutException;
import java.nio.CharBuffer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.time.Duration;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;
import java.util.Set;
import java.util.regex.Pattern;

/**
 * A client of the schemgen2 HTTP API, v2 (see {@code docs/api.md}). Every
 * call blocks, so the mod makes them off the render thread; none of them
 * holds state, so one client serves any number of threads.
 */
public final class BackendClient implements AutoCloseable {
    private static final Duration CONNECT_TIMEOUT = Duration.ofSeconds(5);
    /** Routes that answer from memory. */
    private static final Duration QUICK = Duration.ofSeconds(15);
    /** Uploads, previews (the pipeline runs before the answer) and downloads. */
    private static final Duration SLOW = Duration.ofMinutes(10);
    /** How often a job is polled when its event stream fails. */
    static final Duration POLL_INTERVAL = Duration.ofMillis(800);
    /** Consecutive failed polls tolerated before the server counts as gone. */
    private static final int POLL_RETRIES = 5;

    private static final Pattern JOB_ID = Pattern.compile("[A-Za-z0-9-]+");
    private static final Set<String> JOB_EVENTS = Set.of("progress", "done", "error", "cancelled");

    private final URI base;
    private final String token;
    private final HttpClient http;

    /** A client of the server at {@code base}, sending {@code token} as a bearer token when it is not blank. */
    public BackendClient(URI base, String token) {
        this(base, token, HttpClient.newBuilder()
                .version(HttpClient.Version.HTTP_1_1)
                .connectTimeout(CONNECT_TIMEOUT)
                // The server is on this machine or the LAN; a system proxy would only get in the way.
                .proxy(HttpClient.Builder.NO_PROXY)
                .build());
    }

    BackendClient(URI base, String token, HttpClient http) {
        this.base = base;
        this.token = token == null || token.isBlank() ? null : token.strip();
        this.http = http;
    }

    public URI baseUri() {
        return base;
    }

    public Health health() throws IOException, InterruptedException {
        return sendJson(request("/api/health", QUICK).GET().build(), Health.class);
    }

    public Schema schema() throws IOException, InterruptedException {
        HttpResponse<byte[]> response = send(request("/api/schema", QUICK).GET().build());
        return parse(response, Schema::parse);
    }

    /** Upload one model with its settings; the server answers at once and converts in the background. */
    public StartedJobs startJob(Path model, JsonObject settings) throws IOException, InterruptedException {
        Multipart body = modelUpload(model, settings);
        HttpRequest request = request("/api/jobs", SLOW)
                .header("Content-Type", body.contentType())
                .POST(body.publisher())
                .build();
        return sendJson(request, StartedJobs.class);
    }

    public Job job(String id) throws IOException, InterruptedException {
        return sendJson(request(jobPath(id), QUICK).GET().build(), Job.class);
    }

    /** {@code GET /api/jobs}. */
    private record JobList(List<Job> jobs) {}

    /** Every job the server remembers, newest first. */
    public List<Job> jobs() throws IOException, InterruptedException {
        List<Job> jobs = sendJson(request("/api/jobs", QUICK).GET().build(), JobList.class).jobs();
        return jobs == null ? List.of() : jobs;
    }

    /**
     * Cancel a queued or running job — its event stream then ends with
     * {@code cancelled} — or forget a finished one and delete its files, in
     * which case there is no job left to return.
     */
    public Optional<Job> cancel(String id) throws IOException, InterruptedException {
        HttpResponse<byte[]> response = send(request(jobPath(id), QUICK).DELETE().build());
        if (response.statusCode() == 204) {
            return Optional.empty();
        }
        return Optional.of(parse(response, text -> Json.API.fromJson(text, Job.class)));
    }

    /**
     * Follow a job until it finishes and return its final state, telling
     * {@code listener} about each change. Uses the job's event stream; if the
     * stream cannot be opened or breaks off, polls {@code GET /api/jobs/{id}}
     * every 800 ms instead. Blocks — interrupt the thread to stop following.
     */
    public Job events(String id, JobListener listener) throws IOException, InterruptedException {
        Job last = null;
        try {
            last = stream(id, listener);
        } catch (BackendException e) {
            if (e.status() >= 400 && e.status() < 500) {
                throw e; // the server refused (unknown job, wrong token): polling would too
            }
        } catch (IOException e) {
            if (Thread.currentThread().isInterrupted()) {
                throw interrupted(e);
            }
            // The stream broke; polling picks up from here.
        }
        if (last != null && last.isFinished()) {
            return last;
        }
        return poll(id, listener, last);
    }

    /** Convert without writing a file and get the blocks back — the in-game ghost preview. */
    public PreviewData preview(Path model, JsonObject settings) throws IOException, InterruptedException {
        Multipart body = modelUpload(model, settings);
        HttpRequest request = request("/api/preview", SLOW)
                .header("Content-Type", body.contentType())
                .POST(body.publisher())
                .build();
        return parse(send(request), PreviewData::parse);
    }

    /** The 256 × 256 PNG the server renders for a finished job. */
    public byte[] thumbnail(String id) throws IOException, InterruptedException {
        HttpResponse<byte[]> response = send(request(jobPath(id) + "/thumbnail.png", QUICK)
                .setHeader("Accept", "image/png")
                .GET()
                .build());
        ensureOk(response.statusCode(), response.body());
        return response.body();
    }

    /**
     * Save a finished job's schematic as {@code target}. It is written next to
     * the target first and moved into place, so no one — Litematica included —
     * ever sees half a file there.
     */
    public void download(String id, Path target) throws IOException, InterruptedException {
        HttpRequest request = request(jobPath(id) + "/download", SLOW)
                .setHeader("Accept", "application/octet-stream")
                .GET()
                .build();
        HttpResponse<InputStream> response = send(request, HttpResponse.BodyHandlers.ofInputStream());
        try (InputStream body = response.body()) {
            ensureOk(response.statusCode(), body);
            AtomicFiles.write(target, body::transferTo);
        }
    }

    /** Colors of the blocks a conversion may choose, as {@code 0xRRGGBB} by full block ID. */
    public Map<String, Integer> palette() throws IOException, InterruptedException {
        JsonObject json = sendJson(request("/api/palette", QUICK).GET().build(), JsonObject.class);
        Map<String, Integer> colors = new LinkedHashMap<>();
        for (Map.Entry<String, JsonElement> entry : json.entrySet()) {
            JsonArray rgb = entry.getValue().getAsJsonArray();
            String id = entry.getKey().contains(":") ? entry.getKey() : "minecraft:" + entry.getKey();
            colors.put(id, channel(rgb, 0) << 16 | channel(rgb, 1) << 8 | channel(rgb, 2));
        }
        return colors;
    }

    /** Abort whatever is in flight, event streams included. */
    @Override
    public void close() {
        http.shutdownNow();
    }

    private Job stream(String id, JobListener listener) throws IOException, InterruptedException {
        HttpRequest request = request(jobPath(id) + "/events", null)
                .setHeader("Accept", "text/event-stream")
                .GET()
                .build();
        HttpResponse<InputStream> response = send(request, HttpResponse.BodyHandlers.ofInputStream());
        try (InputStream body = response.body()) {
            ensureOk(response.statusCode(), body);
            Job[] last = {null};
            SseParser parser = new SseParser(event -> {
                if (!JOB_EVENTS.contains(event.event())) {
                    return; // an event type from a newer server
                }
                Job job = parseJob(event.data());
                last[0] = job;
                listener.onUpdate(job);
            });
            Reader reader = new InputStreamReader(body, StandardCharsets.UTF_8);
            char[] buffer = new char[8192];
            int n;
            while ((last[0] == null || !last[0].isFinished()) && (n = reader.read(buffer)) != -1) {
                parser.feed(CharBuffer.wrap(buffer, 0, n));
            }
            return last[0];
        }
    }

    private Job poll(String id, JobListener listener, Job last) throws IOException, InterruptedException {
        int failures = 0;
        while (true) {
            Job job;
            try {
                job = job(id);
                failures = 0;
            } catch (BackendException e) {
                boolean refused = e.status() >= 400 && e.status() < 500;
                if (refused || ++failures > POLL_RETRIES) {
                    throw e;
                }
                Thread.sleep(POLL_INTERVAL.toMillis());
                continue;
            }
            if (!Objects.equals(job, last)) {
                listener.onUpdate(job);
            }
            if (job.isFinished()) {
                return job;
            }
            last = job;
            Thread.sleep(POLL_INTERVAL.toMillis());
        }
    }

    private Multipart modelUpload(Path model, JsonObject settings) {
        return Multipart.withRandomBoundary()
                .addFile("file", model, "application/octet-stream")
                .addText("settings", "application/json; charset=utf-8", Json.API.toJson(settings));
    }

    private HttpRequest.Builder request(String path, Duration timeout) {
        HttpRequest.Builder builder = HttpRequest.newBuilder(base.resolve(path)).header("Accept", "application/json");
        if (timeout != null) {
            builder.timeout(timeout);
        }
        if (token != null) {
            builder.header("Authorization", "Bearer " + token);
        }
        return builder;
    }

    private HttpResponse<byte[]> send(HttpRequest request) throws IOException, InterruptedException {
        return send(request, HttpResponse.BodyHandlers.ofByteArray());
    }

    private <T> HttpResponse<T> send(HttpRequest request, HttpResponse.BodyHandler<T> handler)
            throws IOException, InterruptedException {
        try {
            return http.send(request, handler);
        } catch (ConnectException e) {
            throw new BackendException("Cannot reach the SchemGen server at " + base + " — is it running?", e);
        } catch (HttpTimeoutException e) {
            throw new BackendException("The SchemGen server at " + base + " did not answer in time", e);
        }
    }

    private <T> T sendJson(HttpRequest request, Class<T> type) throws IOException, InterruptedException {
        return parse(send(request), text -> Json.API.fromJson(text, type));
    }

    private interface Parser<T> {
        T parse(String text);
    }

    private static <T> T parse(HttpResponse<byte[]> response, Parser<T> parser) throws BackendException {
        ensureOk(response.statusCode(), response.body());
        try {
            T value = parser.parse(new String(response.body(), StandardCharsets.UTF_8));
            if (value == null) {
                throw new JsonParseException("empty body");
            }
            return value;
        } catch (JsonParseException | IllegalStateException e) {
            throw new BackendException("Unexpected answer from the server: " + e.getMessage(), response.statusCode(), e);
        }
    }

    private static Job parseJob(String json) {
        Job job = Json.API.fromJson(json, Job.class);
        if (job == null || job.id() == null) {
            throw new JsonParseException("event without a job");
        }
        return job;
    }

    private static void ensureOk(int status, byte[] body) throws BackendException {
        if (status / 100 != 2) {
            throw BackendException.fromResponse(status, body);
        }
    }

    private static void ensureOk(int status, InputStream body) throws IOException {
        if (status / 100 != 2) {
            throw BackendException.fromResponse(status, body.readAllBytes());
        }
    }

    private static String jobPath(String id) {
        if (id == null || !JOB_ID.matcher(id).matches()) {
            throw new IllegalArgumentException("Not a job id: " + id);
        }
        return "/api/jobs/" + id;
    }

    private static int channel(JsonArray rgb, int i) {
        return (int) Math.max(0, Math.min(255, Math.round(rgb.get(i).getAsDouble())));
    }

    private static InterruptedException interrupted(IOException cause) {
        InterruptedException e = new InterruptedException("Stopped following the job");
        e.initCause(cause);
        return e;
    }
}
