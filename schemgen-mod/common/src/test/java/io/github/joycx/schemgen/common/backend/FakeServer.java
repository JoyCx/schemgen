package io.github.joycx.schemgen.common.backend;

import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import java.io.IOException;
import java.io.OutputStream;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.Executors;

/** A local HTTP server with scripted routes, standing in for schemgen2 in unit tests. */
final class FakeServer implements AutoCloseable {
    /** What a test saw of one request. */
    record Request(String method, String path, String authorization, String contentType, byte[] body) {}

    @FunctionalInterface
    interface Handler {
        void handle(HttpExchange exchange) throws IOException;
    }

    private final HttpServer server;
    final List<Request> requests = new CopyOnWriteArrayList<>();

    FakeServer() throws IOException {
        server = HttpServer.create(new InetSocketAddress(InetAddress.getLoopbackAddress(), 0), 0);
        server.setExecutor(Executors.newCachedThreadPool());
        server.start();
    }

    URI uri() {
        return URI.create("http://127.0.0.1:" + server.getAddress().getPort());
    }

    /** Route {@code path} (a prefix) to {@code handler}, recording each request first. */
    FakeServer on(String path, Handler handler) {
        server.createContext(path, exchange -> {
            byte[] body = exchange.getRequestBody().readAllBytes();
            requests.add(new Request(
                    exchange.getRequestMethod(),
                    exchange.getRequestURI().getPath(),
                    exchange.getRequestHeaders().getFirst("Authorization"),
                    exchange.getRequestHeaders().getFirst("Content-Type"),
                    body));
            try {
                handler.handle(exchange);
            } finally {
                exchange.close();
            }
        });
        return this;
    }

    static void respond(HttpExchange exchange, int status, String contentType, byte[] body) throws IOException {
        exchange.getResponseHeaders().set("Content-Type", contentType);
        exchange.sendResponseHeaders(status, body.length == 0 ? -1 : body.length);
        if (body.length > 0) {
            try (OutputStream out = exchange.getResponseBody()) {
                out.write(body);
            }
        }
    }

    static void json(HttpExchange exchange, int status, String json) throws IOException {
        respond(exchange, status, "application/json", json.getBytes(StandardCharsets.UTF_8));
    }

    @Override
    public void close() {
        server.stop(0);
    }
}
