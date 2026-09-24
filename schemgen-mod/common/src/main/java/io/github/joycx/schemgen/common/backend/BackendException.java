package io.github.joycx.schemgen.common.backend;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.IOException;
import java.nio.charset.StandardCharsets;

/**
 * Something the server, or starting it, went wrong with — with a message meant
 * for the player. API errors carry the server's own {@code {"error": "..."}}
 * text, which names the field or the problem.
 */
public class BackendException extends IOException {
    private static final long serialVersionUID = 1L;

    /** HTTP status of an API error; 0 when there was no response. */
    private final int status;

    public BackendException(String message) {
        this(message, 0, null);
    }

    public BackendException(String message, Throwable cause) {
        this(message, 0, cause);
    }

    public BackendException(String message, int status, Throwable cause) {
        super(message, cause);
        this.status = status;
    }

    public int status() {
        return status;
    }

    /** The error of a failed API response: its {@code error} field, else the status. */
    static BackendException fromResponse(int status, byte[] body) {
        String text = new String(body, StandardCharsets.UTF_8).strip();
        String message = errorField(text);
        if (message == null) {
            message = text.isEmpty() || text.length() > 300 ? "HTTP " + status : "HTTP " + status + ": " + text;
        }
        return new BackendException(message, status, null);
    }

    private static String errorField(String text) {
        try {
            JsonElement json = JsonParser.parseString(text);
            if (json.isJsonObject()) {
                JsonObject object = json.getAsJsonObject();
                JsonElement error = object.get("error");
                if (error != null && error.isJsonPrimitive()) {
                    return error.getAsString();
                }
            }
        } catch (RuntimeException notJson) {
            // Proxies and crashes answer with plain text or HTML.
        }
        return null;
    }
}
