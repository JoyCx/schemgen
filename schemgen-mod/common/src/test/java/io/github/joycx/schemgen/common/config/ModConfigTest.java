package io.github.joycx.schemgen.common.config;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.net.URI;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class ModConfigTest {
    @TempDir
    Path dir;

    private final List<String> warnings = new ArrayList<>();

    private ModConfig load(Path file) {
        return ModConfig.load(file, warnings::add);
    }

    @Test
    void aMissingFileGivesTheDefaults() {
        ModConfig config = load(dir.resolve(ModConfig.FILE_NAME));
        assertEquals(ServerMode.SIDECAR, config.serverMode);
        assertEquals(3001, config.port);
        assertEquals(64, config.previewMaxSize);
        assertTrue(config.autoLoadIntoLitematica);
        assertEquals(dir.resolve("schematics"), config.outputFolder(dir));
        assertEquals(dir.resolve("schemgen/models"), config.modelsFolder(dir));
        assertTrue(warnings.isEmpty());
    }

    @Test
    void savedSettingsComeBack() throws Exception {
        Path file = dir.resolve("config/schemgen.json");
        ModConfig config = new ModConfig();
        config.serverMode = ServerMode.EXTERNAL;
        config.host = "192.168.1.20";
        config.port = 4000;
        config.token = "abc";
        config.outputFolder = "/srv/schematics";
        config.lastSettings.addProperty("max_size", 96);
        config.save(file);

        ModConfig back = load(file);
        assertEquals(ServerMode.EXTERNAL, back.serverMode);
        assertEquals(URI.create("http://192.168.1.20:4000"), back.externalUri());
        assertEquals("abc", back.token);
        assertEquals(Path.of("/srv/schematics"), back.outputFolder(dir));
        back.modelsFolder = "models/mine";
        assertEquals(dir.resolve("models/mine"), back.modelsFolder(dir), "relative to the game directory");

        back.outputFolder = "no\u0000such path"; // a NUL is not allowed in a path anywhere
        assertEquals(dir.resolve("schematics"), back.sanitized().outputFolder(dir), "an unusable folder is the default");
        assertEquals(96, back.lastSettings.get("max_size").getAsInt());
        assertTrue(Files.readString(file).contains("\n  \"serverMode\": \"EXTERNAL\""), "readable JSON");
    }

    @Test
    void partialAndOddFilesStillLoad() throws Exception {
        Path file = Files.writeString(dir.resolve("schemgen.json"),
                "{\"port\": 0, \"previewMaxSize\": 999, \"serverMode\": \"BOGUS\", \"host\": null, \"lastSettings\": null}");
        ModConfig config = load(file);
        assertEquals(3001, config.port);
        assertEquals(256, config.previewMaxSize);
        assertEquals(ServerMode.SIDECAR, config.serverMode);
        assertEquals("127.0.0.1", config.host);
        assertEquals(0, config.lastSettings.size());
        assertTrue(config.autoLoadIntoLitematica, "a missing field keeps its default");
        assertTrue(warnings.isEmpty(), warnings.toString());
        assertTrue(Files.exists(file), "a file that loads is left alone");
    }

    @Test
    void aBrokenFileIsKeptAsideAndTheDefaultsUsed() throws Exception {
        Path file = Files.writeString(dir.resolve("schemgen.json"), "{ \"port\": 40");
        ModConfig config = load(file);
        assertEquals(3001, config.port);
        assertFalse(Files.exists(file));
        assertEquals("{ \"port\": 40", Files.readString(dir.resolve("schemgen.json.broken")));
        assertEquals(1, warnings.size());
    }

    @Test
    void ipv6HostsAreBracketed() {
        ModConfig config = new ModConfig();
        config.host = "::1";
        assertEquals(URI.create("http://[::1]:3001"), config.externalUri());
    }

    @Test
    void hostsThatCannotBeConnectedToAreReplaced() {
        assertTrue(ModConfig.isValidHost("my-pc.local"));
        assertTrue(ModConfig.isValidHost("192.168.1.20"));
        assertTrue(ModConfig.isValidHost("[::1]"));
        assertFalse(ModConfig.isValidHost("my pc"));
        assertFalse(ModConfig.isValidHost("under_score"));
        assertFalse(ModConfig.isValidHost("http://my-pc"));

        ModConfig config = new ModConfig();
        config.host = "my pc";
        assertEquals("127.0.0.1", config.sanitized().host);
    }
}
