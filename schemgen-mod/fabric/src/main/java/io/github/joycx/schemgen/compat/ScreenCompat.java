package io.github.joycx.schemgen.compat;

import io.github.joycx.schemgen.SchemGenClient;
import net.minecraft.client.gui.DrawContext;
import net.minecraft.client.option.KeyBinding;
import net.minecraft.client.texture.NativeImage;
import net.minecraft.client.texture.NativeImageBackedTexture;
import net.minecraft.util.Identifier;
//? if >=1.21.6 {
import net.minecraft.client.gl.RenderPipelines;
//?} else if >=1.21.2 {
/*import net.minecraft.client.render.RenderLayer;
*///?}

/**
 * The GUI calls whose signatures differ between the Minecraft versions the
 * mod is built for. Every other GUI call it makes — widgets, text, fills —
 * is the same in all of them. Each boundary below was read off the Yarn
 * mappings of the versions on both sides of it (see docs/mod.md).
 */
public final class ScreenCompat {
    private ScreenCompat() {}

    //? if >=1.21.9 {
    /*private static final KeyBinding.Category CATEGORY =
            KeyBinding.Category.create(Identifier.of(SchemGenClient.MOD_ID, "main"));
    *///?}

    /** A key binding in the mod's own category of the Controls screen. */
    public static KeyBinding keyBinding(String translationKey, int glfwKey) {
        //? if >=1.21.9 {
        /*return new KeyBinding(translationKey, glfwKey, CATEGORY);
        *///?} else {
        return new KeyBinding(translationKey, glfwKey, "key.categories." + SchemGenClient.MOD_ID);
        //?}
    }

    /** Draw a whole texture scaled into a {@code width} × {@code height} box. */
    public static void drawTexture(DrawContext context, Identifier texture, int x, int y, int width, int height) {
        //? if >=1.21.6 {
        context.drawTexture(RenderPipelines.GUI_TEXTURED, texture, x, y, 0, 0, width, height, width, height);
        //?} else if >=1.21.2 {
        /*context.drawTexture(RenderLayer::getGuiTextured, texture, x, y, 0, 0, width, height, width, height);
        *///?} else {
        /*context.drawTexture(texture, x, y, 0, 0, width, height, width, height);
        *///?}
    }

    /** A texture holding {@code image}, for the texture manager. Render thread only. */
    public static NativeImageBackedTexture texture(String name, NativeImage image) {
        //? if >=1.21.5 {
        return new NativeImageBackedTexture(() -> name, image);
        //?} else {
        /*return new NativeImageBackedTexture(image);
        *///?}
    }
}
