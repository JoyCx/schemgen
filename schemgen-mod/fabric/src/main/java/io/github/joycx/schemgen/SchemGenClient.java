package io.github.joycx.schemgen;

import io.github.joycx.schemgen.compat.ScreenCompat;
import io.github.joycx.schemgen.preview.GhostRenderer;
import io.github.joycx.schemgen.ui.SchemGenScreen;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientLifecycleEvents;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.fabric.api.client.keybinding.v1.KeyBindingHelper;
import net.minecraft.client.option.KeyBinding;
import org.lwjgl.glfw.GLFW;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Entry point: the key bindings, the session, the ghost renderer, and
 * stopping the server with the game. Nothing starts the server here — the
 * screen does, the first time it opens.
 */
public final class SchemGenClient implements ClientModInitializer {
    public static final String MOD_ID = "schemgen";
    public static final Logger LOGGER = LoggerFactory.getLogger("SchemGen");

    @Override
    public void onInitializeClient() {
        SchemGenSession session = SchemGenSession.start();

        KeyBinding open = KeyBindingHelper.registerKeyBinding(ScreenCompat.keyBinding("key.schemgen.open", GLFW.GLFW_KEY_K));
        // Unbound by default: single letters are contested, and the screen has buttons for both.
        KeyBinding rotate = KeyBindingHelper.registerKeyBinding(
                ScreenCompat.keyBinding("key.schemgen.rotate_preview", GLFW.GLFW_KEY_UNKNOWN));
        KeyBinding clear = KeyBindingHelper.registerKeyBinding(
                ScreenCompat.keyBinding("key.schemgen.clear_preview", GLFW.GLFW_KEY_UNKNOWN));

        ClientTickEvents.END_CLIENT_TICK.register(client -> {
            while (open.wasPressed()) {
                client.setScreen(new SchemGenScreen(session));
            }
            while (rotate.wasPressed()) {
                session.preview().rotate();
            }
            while (clear.wasPressed()) {
                session.preview().clear();
            }
        });
        ClientLifecycleEvents.CLIENT_STOPPING.register(client -> session.shutdown());
        GhostRenderer.register(session.preview());
    }
}
