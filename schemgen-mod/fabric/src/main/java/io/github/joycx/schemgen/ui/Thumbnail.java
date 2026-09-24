package io.github.joycx.schemgen.ui;

import io.github.joycx.schemgen.SchemGenClient;
import io.github.joycx.schemgen.compat.ScreenCompat;
import java.io.IOException;
import net.minecraft.client.MinecraftClient;
import net.minecraft.client.texture.NativeImage;
import net.minecraft.util.Identifier;

/**
 * The last conversion's thumbnail — the server's isometric render — as a
 * texture the screen can draw. Render thread only.
 */
public final class Thumbnail {
    private Identifier id;
    private int generation;

    /** Show {@code png} instead of the previous thumbnail; one the game cannot read leaves none. */
    public void set(byte[] png) {
        clear();
        try {
            NativeImage image = NativeImage.read(png);
            // A fresh id per image, so nothing can keep drawing the old one.
            Identifier next = Identifier.of(SchemGenClient.MOD_ID, "thumbnail/" + ++generation);
            MinecraftClient.getInstance().getTextureManager().registerTexture(next, ScreenCompat.texture(next.toString(), image));
            id = next;
        } catch (IOException e) {
            SchemGenClient.LOGGER.warn("Unreadable thumbnail from the server", e);
        }
    }

    public void clear() {
        if (id != null) {
            MinecraftClient.getInstance().getTextureManager().destroyTexture(id);
            id = null;
        }
    }

    /** The texture to draw, or {@code null} when there is none. */
    public Identifier id() {
        return id;
    }
}
