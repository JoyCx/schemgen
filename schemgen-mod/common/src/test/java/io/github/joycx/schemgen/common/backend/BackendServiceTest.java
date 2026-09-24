package io.github.joycx.schemgen.common.backend;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;

import io.github.joycx.schemgen.common.config.ModConfig;
import io.github.joycx.schemgen.common.config.ServerMode;
import java.nio.file.Path;
import java.util.Optional;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class BackendServiceTest {
    @TempDir
    Path gameDir;

    private final ModConfig config = new ModConfig();

    private BackendService service() {
        ServerBinaries none = new ServerBinaries(null, Optional.empty(), path -> null, (uri, target) -> {});
        return new BackendService(() -> config, gameDir, none, new BackendLauncher(line -> {}), line -> {});
    }

    @Test
    void externalModeChecksTheServerAndKeepsItsClient() throws Exception {
        try (FakeServer server = new FakeServer().on("/api/health", ex -> FakeServer.json(ex, 200,
                "{\"status\":\"ok\",\"name\":\"schemgen2\",\"version\":\"2.0.0\",\"api\":2,\"auth\":false}"))) {
            config.serverMode = ServerMode.EXTERNAL;
            config.port = server.uri().getPort();
            try (BackendService service = service()) {
                BackendClient client = service.connect();
                assertEquals(server.uri(), client.baseUri());
                assertSame(client, service.connect());
            }
        }
    }

    @Test
    void anOldServerIsTurnedDown() throws Exception {
        try (FakeServer server = new FakeServer().on("/api/health", ex -> FakeServer.json(ex, 200,
                "{\"status\":\"ok\",\"version\":\"2.0.0-beta\"}"))) {
            config.serverMode = ServerMode.EXTERNAL;
            config.port = server.uri().getPort();
            try (BackendService service = service()) {
                BackendException e = assertThrows(BackendException.class, service::connect);
                assertEquals("The server at " + server.uri() + " is schemgen2 2.0.0-beta, which is too old for the mod: "
                        + "it needs API v2 (schemgen2 2.1 or later)", e.getMessage());
            }
        }
    }

    @Test
    void sidecarModeExplainsWhyThereIsNoServer() {
        try (BackendService service = service()) {
            BackendException e = assertThrows(BackendException.class, service::connect);
            assertEquals("SchemGen server did not start: This build of SchemGen pins no server release. "
                    + "Start `schemgen2 serve` yourself and switch SchemGen to an external server.", e.getMessage());
        }
    }
}
