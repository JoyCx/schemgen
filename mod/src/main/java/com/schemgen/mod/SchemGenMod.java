package com.schemgen.mod;

import com.schemgen.mod.gui.SchemGenScreen;

import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.minecraft.client.MinecraftClient;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Client entrypoint.
 *
 * <p>The mod is a thin front end: it never converts anything itself. It talks
 * to the SchemGen2 HTTP API (the same one the web UI uses, started with
 * {@code schemgen2 serve}) and drops the finished {@code .litematic} into this
 * instance's own {@code schematics} folder, where Litematica finds it.
 *
 * <p>Nothing here touches the world or the server connection, so the mod is
 * client-only and safe on multiplayer servers.
 */
public final class SchemGenMod implements ClientModInitializer {

    public static final String MOD_ID = "schemgen";
    public static final Logger LOGGER = LoggerFactory.getLogger("SchemGen2");

    private static SchemGenConfig config = SchemGenConfig.defaults();

    /** Set from a command; the screen opens on the next tick — see below. */
    private static boolean openScreenRequested;

    @Override
    public void onInitializeClient() {
        config = SchemGenConfig.load();
        SchemGenCommands.register();

        // Opening a Screen straight from a command handler does not stick: the
        // chat screen is still closing, and its close overwrites whatever the
        // command opened. Deferring one tick is the standard fix.
        ClientTickEvents.END_CLIENT_TICK.register(client -> {
            if (openScreenRequested) {
                openScreenRequested = false;
                client.setScreen(new SchemGenScreen());
            }
        });

        LOGGER.info("SchemGen2 client ready — API at {}", config.serverUrl);
    }

    public static SchemGenConfig config() {
        return config;
    }

    /** Open the converter GUI on the next client tick. */
    public static void openScreen() {
        openScreenRequested = true;
    }

    /** Run something on the client thread, for callers on a worker thread. */
    public static void onClientThread(Runnable action) {
        MinecraftClient.getInstance().execute(action);
    }
}
