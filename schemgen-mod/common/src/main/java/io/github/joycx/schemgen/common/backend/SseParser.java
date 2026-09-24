package io.github.joycx.schemgen.common.backend;

import java.util.function.Consumer;

/**
 * An incremental {@code text/event-stream} parser, following the WHATWG
 * event-stream rules: text arrives in arbitrary pieces (a line, a CRLF pair
 * or a UTF-16 surrogate pair may be split between two), lines end with CRLF,
 * LF or CR, {@code :} starts a comment, a blank line dispatches the event,
 * and several {@code data:} lines join with {@code \n}.
 */
public final class SseParser {
    /** One dispatched event. {@code event} is {@code "message"} when the stream named none. */
    public record Event(String event, String data, String id) {}

    private final Consumer<Event> sink;
    private final StringBuilder line = new StringBuilder();
    private final StringBuilder data = new StringBuilder();
    private String eventType = "";
    private String lastEventId = "";
    private long retryMillis = -1;
    private boolean lastWasCarriageReturn;
    private boolean started;

    public SseParser(Consumer<Event> sink) {
        this.sink = sink;
    }

    /** Parse the next piece of the stream, dispatching every event it completes. */
    public void feed(CharSequence chunk) {
        for (int i = 0; i < chunk.length(); i++) {
            char c = chunk.charAt(i);
            if (!started) {
                started = true;
                if (c == '﻿') {
                    continue; // a byte-order mark may open the stream
                }
            }
            if (lastWasCarriageReturn) {
                lastWasCarriageReturn = false;
                if (c == '\n') {
                    continue; // the LF of a CRLF pair
                }
            }
            if (c == '\r') {
                lastWasCarriageReturn = true;
                processLine();
            } else if (c == '\n') {
                processLine();
            } else {
                line.append(c);
            }
        }
    }

    /**
     * The reconnection delay the stream asked for with {@code retry:}, or -1.
     * It is a hint for reconnecting clients; {@link BackendClient} falls back
     * to polling at its own pace instead of reconnecting.
     */
    public long retryMillis() {
        return retryMillis;
    }

    public String lastEventId() {
        return lastEventId;
    }

    private void processLine() {
        String text = line.toString();
        line.setLength(0);
        if (text.isEmpty()) {
            dispatch();
            return;
        }
        if (text.charAt(0) == ':') {
            return; // comment, e.g. the server's ": keep-alive"
        }
        int colon = text.indexOf(':');
        String field = colon < 0 ? text : text.substring(0, colon);
        String value = colon < 0 ? "" : text.substring(colon + 1);
        if (value.startsWith(" ")) {
            value = value.substring(1);
        }
        switch (field) {
            case "event" -> eventType = value;
            case "data" -> data.append(value).append('\n');
            case "id" -> {
                if (value.indexOf('\0') < 0) {
                    lastEventId = value;
                }
            }
            case "retry" -> {
                if (!value.isEmpty() && value.chars().allMatch(ch -> ch >= '0' && ch <= '9')) {
                    try {
                        retryMillis = Long.parseLong(value);
                    } catch (NumberFormatException tooLong) {
                        // Ignored, as the spec says for any invalid value.
                    }
                }
            }
            default -> {
                // Unknown fields are ignored.
            }
        }
    }

    private void dispatch() {
        if (data.isEmpty()) {
            eventType = ""; // e.g. the blank line after the opening "retry:"
            return;
        }
        data.setLength(data.length() - 1); // the last line's '\n'
        Event event = new Event(eventType.isEmpty() ? "message" : eventType, data.toString(), lastEventId);
        data.setLength(0);
        eventType = "";
        sink.accept(event);
    }
}
