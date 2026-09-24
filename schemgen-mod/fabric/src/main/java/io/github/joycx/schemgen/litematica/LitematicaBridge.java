package io.github.joycx.schemgen.litematica;

import fi.dy.masa.litematica.data.DataManager;
import fi.dy.masa.litematica.data.SchematicHolder;
import fi.dy.masa.litematica.schematic.LitematicaSchematic;
import fi.dy.masa.litematica.schematic.placement.SchematicPlacement;
import fi.dy.masa.litematica.schematic.placement.SchematicPlacementManager;
import fi.dy.masa.malilib.gui.Message.MessageType;
import fi.dy.masa.malilib.gui.interfaces.IMessageConsumer;
import fi.dy.masa.malilib.util.InfoUtils;
import java.nio.file.Path;
import net.minecraft.text.Text;
import net.minecraft.util.BlockRotation;
import net.minecraft.util.math.BlockPos;

/**
 * Litematica's internals — loading a schematic, placing it, moving and
 * removing a placement — the way Litematica's own "Load schematic" screen
 * does it. The only class that imports Litematica; reached through
 * {@link LitematicaSupport} only when Litematica is installed. Written
 * against sakura-ryoko's Litematica at the release each Minecraft version
 * compiles against (see fabric/versions/&lt;version&gt;/gradle.properties).
 */
final class LitematicaBridge {
    private static final String PREVIEW_NAME = "SchemGen preview";

    /** Litematica's own on-screen messages for placement changes (e.g. "placement is locked"). */
    private static final IMessageConsumer FEEDBACK = new IMessageConsumer() {
        @Override
        public void addMessage(MessageType type, String messageKey, Object... args) {
            InfoUtils.showGuiOrInGameMessage(type, messageKey, args);
        }

        @Override
        public void addMessage(MessageType type, int lifeTime, String messageKey, Object... args) {
            InfoUtils.showGuiOrInGameMessage(type, lifeTime, messageKey, args);
        }
    };

    private static SchematicPlacement preview;
    private static LitematicaSchematic previewSchematic;

    private LitematicaBridge() {}

    static Text place(Path file, BlockPos origin, int quarterTurns) {
        LitematicaSchematic schematic = read(file);
        if (schematic == null) {
            return Text.translatable("schemgen.litematica.unreadable", file.getFileName().toString());
        }
        SchematicHolder.getInstance().addSchematic(schematic, true);
        place(schematic, origin, schematic.getMetadata().getName(), quarterTurns);
        return null;
    }

    static Text showPreview(Path file, BlockPos origin, int quarterTurns) {
        clearPreview();
        LitematicaSchematic schematic = read(file);
        if (schematic == null) {
            return Text.translatable("schemgen.litematica.unreadable", file.getFileName().toString());
        }
        SchematicHolder.getInstance().addSchematic(schematic, true);
        previewSchematic = schematic;
        preview = place(schematic, origin, PREVIEW_NAME, quarterTurns);
        return null;
    }

    static void movePreview(BlockPos origin, int quarterTurns) {
        if (preview != null) {
            preview.setRotation(rotation(quarterTurns), FEEDBACK);
            preview.setOrigin(origin, InfoUtils.INFO_MESSAGE_CONSUMER);
        }
    }

    static void clearPreview() {
        if (preview != null) {
            DataManager.getSchematicPlacementManager().removeSchematicPlacement(preview);
            preview = null;
        }
        if (previewSchematic != null) {
            SchematicHolder.getInstance().removeSchematic(previewSchematic);
            previewSchematic = null;
        }
    }

    private static SchematicPlacement place(LitematicaSchematic schematic, BlockPos origin, String name, int quarterTurns) {
        SchematicPlacement placement = SchematicPlacement.createFor(schematic, origin, name, true, true);
        SchematicPlacementManager manager = DataManager.getSchematicPlacementManager();
        manager.addSchematicPlacement(placement, true);
        manager.setSelectedSchematicPlacement(placement);
        // Turned once placed, as a player turning it in Litematica would.
        if (Math.floorMod(quarterTurns, 4) != 0) {
            placement.setRotation(rotation(quarterTurns), FEEDBACK);
        }
        return placement;
    }

    /** The schematic in {@code file}, or {@code null} when Litematica cannot read it. */
    private static LitematicaSchematic read(Path file) {
        String name = file.getFileName().toString();
        // Litematica moved from File to Path folders between its 1.21.4 and 1.21.8 lines.
        //? if >=1.21.8 {
        return LitematicaSchematic.createFromFile(file.toAbsolutePath().getParent(), name);
        //?} else {
        /*return LitematicaSchematic.createFromFile(file.toAbsolutePath().getParent().toFile(), name);
        *///?}
    }

    /** Clockwise quarter turns seen from above, as Litematica's placement rotation. */
    private static BlockRotation rotation(int quarterTurns) {
        return switch (Math.floorMod(quarterTurns, 4)) {
            case 1 -> BlockRotation.CLOCKWISE_90;
            case 2 -> BlockRotation.CLOCKWISE_180;
            case 3 -> BlockRotation.COUNTERCLOCKWISE_90;
            default -> BlockRotation.NONE;
        };
    }
}
