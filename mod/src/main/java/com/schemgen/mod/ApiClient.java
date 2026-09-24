package com.schemgen.mod;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.LinkedHashMap;
import java.util.Map;

/**
 * The SchemGen2 HTTP API, as the mod uses it.
 *
 * <p>Four calls make a conversion: {@link #health}, {@link #startConversion},
 * {@link #progress} until it finishes, then {@link #download}. The routes and
 * their required fields are documented in {@code docs/api.md}; the multipart
 * fields marked mandatory there are always sent, even when empty, because the
 * server rejects the request otherwise.
 *
 * <p>Built on {@code java.net.http}, which is in the JDK Minecraft already
 * runs on — the mod pulls in no HTTP library of its own.
 */
public final class ApiClient {

    private static final String BOUNDARY = "----SchemGen2FormBoundary";

    private final HttpClient http = HttpClient.newBuilder()
            .connectTimeout(Duration.ofSeconds(5))
            .followRedirects(HttpClient.Redirect.NORMAL)
            .build();

    /** What {@code GET /api/health} reports about a running server. */
    public record Health(String version, int paletteBlocks, int dataVersion) {}

    /** One poll of {@code GET /api/progress/{job}}. */
    public record Progress(String status, float percent, String message) {
        public boolean done() {
            return "done".equals(status);
        }

        public boolean failed() {
            return "error".equals(status);
        }
    }

    /** A conversion request, in the units the API expects. */
    public record Settings(int maxSize, boolean dither, float delight, String schematicName) {}

    /** Thrown for anything that stops a conversion, with a message fit for the GUI. */
    public static final class ApiException extends IOException {
        public ApiException(String message) {
            super(message);
        }
    }

    /**
     * Ping the server. Throws {@link ApiException} with a message the GUI can
     * show verbatim when it is not reachable.
     */
    public Health health(String baseUrl) throws ApiException, InterruptedException {
        HttpRequest request = HttpRequest.newBuilder(uri(baseUrl, "/api/health"))
                .timeout(Duration.ofSeconds(5))
                .GET()
                .build();
        try {
            HttpResponse<String> response = http.send(request, HttpResponse.BodyHandlers.ofString());
            if (response.statusCode() != 200) {
                throw new ApiException("Server answered HTTP " + response.statusCode() + " on /api/health");
            }
            JsonObject json = JsonParser.parseString(response.body()).getAsJsonObject();
            return new Health(
                    json.get("version").getAsString(),
                    json.get("palette_blocks").getAsInt(),
                    json.get("data_version").getAsInt());
        } catch (IOException e) {
            throw new ApiException("No SchemGen2 server at " + baseUrl
                    + " — start it with `schemgen2 serve` (" + e.getMessage() + ")");
        } catch (RuntimeException e) {
            throw new ApiException("Unexpected /api/health reply from " + baseUrl + ": " + e);
        }
    }

    /** Upload a model and start a conversion. Returns the job id. */
    public String startConversion(String baseUrl, Path model, Settings settings)
            throws ApiException, InterruptedException {

        byte[] body;
        try {
            body = multipartBody(model, settings);
        } catch (IOException e) {
            throw new ApiException("Could not read " + model.getFileName() + ": " + e.getMessage());
        }

        HttpRequest request = HttpRequest.newBuilder(uri(baseUrl, "/api/convert"))
                .header("Content-Type", "multipart/form-data; boundary=" + BOUNDARY)
                // Uploads of a few hundred MB over loopback are still quick, but
                // a slow disk on the server side can stretch this out.
                .timeout(Duration.ofMinutes(10))
                .POST(HttpRequest.BodyPublishers.ofByteArray(body))
                .build();

        HttpResponse<String> response = send(request, "start the conversion");
        if (response.statusCode() != 200) {
            throw new ApiException("Conversion refused: " + errorMessage(response));
        }
        JsonObject json = JsonParser.parseString(response.body()).getAsJsonObject();
        if (!json.has("job_id")) {
            throw new ApiException("Server did not return a job id");
        }
        return json.get("job_id").getAsString();
    }

    public Progress progress(String baseUrl, String jobId) throws ApiException, InterruptedException {
        HttpRequest request = HttpRequest.newBuilder(uri(baseUrl, "/api/progress/" + jobId))
                .timeout(Duration.ofSeconds(15))
                .GET()
                .build();

        HttpResponse<String> response = send(request, "read progress");
        if (response.statusCode() != 200) {
            throw new ApiException("Lost track of the job: " + errorMessage(response));
        }
        JsonObject json = JsonParser.parseString(response.body()).getAsJsonObject();
        return new Progress(
                json.get("status").getAsString(),
                json.has("progress") ? json.get("progress").getAsFloat() : 0.0f,
                json.has("message") ? json.get("message").getAsString() : "");
    }

    /** Fetch the finished schematic's bytes. */
    public byte[] download(String baseUrl, String jobId) throws ApiException, InterruptedException {
        HttpRequest request = HttpRequest.newBuilder(uri(baseUrl, "/api/download/" + jobId))
                .timeout(Duration.ofMinutes(2))
                .GET()
                .build();
        try {
            HttpResponse<byte[]> response = http.send(request, HttpResponse.BodyHandlers.ofByteArray());
            if (response.statusCode() != 200) {
                throw new ApiException("Download failed with HTTP " + response.statusCode());
            }
            return response.body();
        } catch (IOException e) {
            throw new ApiException("Could not download the schematic: " + e.getMessage());
        }
    }

    // ---- plumbing ----------------------------------------------------------

    private HttpResponse<String> send(HttpRequest request, String what)
            throws ApiException, InterruptedException {
        try {
            return http.send(request, HttpResponse.BodyHandlers.ofString());
        } catch (IOException e) {
            throw new ApiException("Could not " + what + ": " + e.getMessage());
        }
    }

    /** Pull `error` out of a JSON error body, falling back to the status line. */
    private static String errorMessage(HttpResponse<String> response) {
        try {
            JsonObject json = JsonParser.parseString(response.body()).getAsJsonObject();
            if (json.has("error")) {
                return json.get("error").getAsString();
            }
        } catch (RuntimeException ignored) {
            // Not JSON — fall through to the status code.
        }
        return "HTTP " + response.statusCode();
    }

    private static URI uri(String baseUrl, String path) {
        return URI.create(baseUrl + path);
    }

    /**
     * Build the {@code multipart/form-data} body by hand.
     *
     * <p>{@code java.net.http} has no multipart publisher, and the whole body
     * is a byte array rather than a stream because the model has to be in
     * memory to be hashed into the request anyway.
     */
    private static byte[] multipartBody(Path model, Settings settings) throws IOException {
        // Mandatory fields, in the order api.rs declares them. `voxel_size`
        // stays empty so the pitch is derived from max_size.
        Map<String, String> fields = new LinkedHashMap<>();
        fields.put("max_size", Integer.toString(settings.maxSize()));
        fields.put("voxel_size", "");
        fields.put("ram_limit", "4");
        fields.put("dither", Boolean.toString(settings.dither()));
        fields.put("color_sampling", "true");
        fields.put("brightness", "0");
        fields.put("contrast", "1");
        fields.put("saturation", "1");
        fields.put("no_color_block", "white");
        fields.put("schematic_name", settings.schematicName());
        fields.put("delight", Float.toString(settings.delight()));
        // The mod saves the file itself, so the server must not also write it
        // into a folder of its own choosing.
        fields.put("auto_save", "false");

        ByteArrayOutputStream out = new ByteArrayOutputStream();
        for (Map.Entry<String, String> field : fields.entrySet()) {
            out.write(ascii("--" + BOUNDARY + "\r\n"));
            out.write(utf8("Content-Disposition: form-data; name=\"" + field.getKey() + "\"\r\n\r\n"));
            out.write(utf8(field.getValue()));
            out.write(ascii("\r\n"));
        }

        String filename = model.getFileName().toString().replace("\"", "");
        out.write(ascii("--" + BOUNDARY + "\r\n"));
        out.write(utf8("Content-Disposition: form-data; name=\"file\"; filename=\"" + filename + "\"\r\n"));
        out.write(ascii("Content-Type: model/gltf-binary\r\n\r\n"));
        out.write(Files.readAllBytes(model));
        out.write(ascii("\r\n"));
        out.write(ascii("--" + BOUNDARY + "--\r\n"));
        return out.toByteArray();
    }

    private static byte[] ascii(String s) {
        return s.getBytes(StandardCharsets.US_ASCII);
    }

    private static byte[] utf8(String s) {
        return s.getBytes(StandardCharsets.UTF_8);
    }
}
