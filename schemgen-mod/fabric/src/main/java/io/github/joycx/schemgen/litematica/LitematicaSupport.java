package io.github.joycx.schemgen.litematica;

import java.nio.file.Path;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.text.Text;
import net.minecraft.util.math.BlockPos;

/**
 * The mod's one way to Litematica, safe whether it is installed or not.
 * {@link LitematicaBridge}, which imports Litematica, is only loaded once
 * {@link #isLoaded()} is true — the JVM loads a class on its first use, and
 * this class's signatures name no Litematica type. Call on the client thread.
 */
public final class LitematicaSupport {
    private static final boolean LOADED = FabricLoader.getInstance().isModLoaded("litematica");

    private LitematicaSupport() {}

    public static boolean isLoaded() {
        return LOADED;
    }

    /** Load a schematic into Litematica and place it, selected. Returns why not, or {@code null}. */
    public static Text place(Path schematic, BlockPos origin, int quarterTurns) {
        return LOADED ? LitematicaBridge.place(schematic, origin, quarterTurns)
                : Text.translatable("schemgen.litematica.missing");
    }

    /** Show a preview schematic as a temporary placement. Returns why not, or {@code null}. */
    public static Text showPreview(Path schematic, BlockPos origin, int quarterTurns) {
        return LOADED ? LitematicaBridge.showPreview(schematic, origin, quarterTurns)
                : Text.translatable("schemgen.litematica.missing");
    }

    public static void movePreview(BlockPos origin, int quarterTurns) {
        if (LOADED) {
            LitematicaBridge.movePreview(origin, quarterTurns);
        }
    }

    public static void clearPreview() {
        if (LOADED) {
            LitematicaBridge.clearPreview();
        }
    }
}
