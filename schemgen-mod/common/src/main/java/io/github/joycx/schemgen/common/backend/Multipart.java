package io.github.joycx.schemgen.common.backend;

import java.io.ByteArrayOutputStream;
import java.io.FileNotFoundException;
import java.io.IOException;
import java.net.http.HttpRequest;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.SecureRandom;
import java.util.ArrayList;
import java.util.HexFormat;
import java.util.List;

/**
 * A {@code multipart/form-data} body: model files streamed from disk (they can
 * be hundreds of megabytes) and small text parts such as the settings JSON.
 * Field and file names are escaped the way browsers do — {@code "}, CR and LF
 * percent-encoded — and sent as UTF-8, which the server reads.
 */
final class Multipart {
    private sealed interface Part permits TextPart, FilePart {}

    private record TextPart(String name, String contentType, String text) implements Part {}

    private record FilePart(String name, Path file, String contentType) implements Part {}

    private final String boundary;
    private final List<Part> parts = new ArrayList<>();

    Multipart(String boundary) {
        this.boundary = boundary;
    }

    /** A body whose boundary cannot occur in a model file by chance. */
    static Multipart withRandomBoundary() {
        byte[] random = new byte[16];
        new SecureRandom().nextBytes(random);
        return new Multipart("----SchemGen" + HexFormat.of().formatHex(random));
    }

    Multipart addFile(String name, Path file, String contentType) {
        parts.add(new FilePart(name, file, contentType));
        return this;
    }

    Multipart addText(String name, String contentType, String text) {
        parts.add(new TextPart(name, contentType, text));
        return this;
    }

    String contentType() {
        return "multipart/form-data; boundary=" + boundary;
    }

    /** The body for {@link java.net.http.HttpClient}; files are read as it is sent. */
    HttpRequest.BodyPublisher publisher() throws FileNotFoundException {
        List<HttpRequest.BodyPublisher> pieces = new ArrayList<>();
        for (Part part : parts) {
            pieces.add(HttpRequest.BodyPublishers.ofByteArray(header(part)));
            switch (part) {
                case TextPart text -> pieces.add(HttpRequest.BodyPublishers.ofByteArray(utf8(text.text())));
                case FilePart file -> pieces.add(HttpRequest.BodyPublishers.ofFile(file.file()));
            }
            pieces.add(HttpRequest.BodyPublishers.ofByteArray(utf8("\r\n")));
        }
        pieces.add(HttpRequest.BodyPublishers.ofByteArray(closing()));
        return HttpRequest.BodyPublishers.concat(pieces.toArray(HttpRequest.BodyPublisher[]::new));
    }

    /** The whole body in memory — the same bytes {@link #publisher()} sends. */
    byte[] toBytes() throws IOException {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        for (Part part : parts) {
            out.write(header(part));
            switch (part) {
                case TextPart text -> out.write(utf8(text.text()));
                case FilePart file -> out.write(Files.readAllBytes(file.file()));
            }
            out.write(utf8("\r\n"));
        }
        out.write(closing());
        return out.toByteArray();
    }

    private byte[] header(Part part) {
        StringBuilder h = new StringBuilder().append("--").append(boundary).append("\r\n");
        switch (part) {
            case TextPart text -> h.append("Content-Disposition: form-data; name=\"")
                    .append(escape(text.name()))
                    .append("\"\r\nContent-Type: ")
                    .append(text.contentType());
            case FilePart file -> h.append("Content-Disposition: form-data; name=\"")
                    .append(escape(file.name()))
                    .append("\"; filename=\"")
                    .append(escape(file.file().getFileName().toString()))
                    .append("\"\r\nContent-Type: ")
                    .append(file.contentType());
        }
        return utf8(h.append("\r\n\r\n").toString());
    }

    private byte[] closing() {
        return utf8("--" + boundary + "--\r\n");
    }

    /** What browsers do to names in a Content-Disposition header. */
    static String escape(String value) {
        return value.replace("\"", "%22").replace("\r", "%0D").replace("\n", "%0A");
    }

    private static byte[] utf8(String s) {
        return s.getBytes(StandardCharsets.UTF_8);
    }
}
