package io.github.joycx.schemgen.preview;

import com.google.gson.JsonObject;
import io.github.joycx.schemgen.SchemGenClient;
import io.github.joycx.schemgen.SchemGenSession;
import io.github.joycx.schemgen.common.backend.BackendClient;
import io.github.joycx.schemgen.common.backend.Conversions;
import io.github.joycx.schemgen.common.model.Job;
import io.github.joycx.schemgen.common.preview.Footprint;
import io.github.joycx.schemgen.common.preview.GhostMesh;
import io.github.joycx.schemgen.common.preview.PreviewData;
import io.github.joycx.schemgen.common.preview.PreviewGrid;
import io.github.joycx.schemgen.common.settings.SettingsModel;
import io.github.joycx.schemgen.litematica.LitematicaSupport;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;
import net.minecraft.client.MinecraftClient;
import net.minecraft.text.Text;
import net.minecraft.util.Formatting;
import net.minecraft.util.hit.BlockHitResult;
import net.minecraft.util.hit.HitResult;
import net.minecraft.util.math.BlockPos;

/**
 * The in-game preview of a conversion, placed on the block the player looks
 * at and turnable in quarter turns.
 *
 * <p>With Litematica installed, the preview is a real, temporary Litematica
 * placement of the model converted at preview size — Litematica already draws
 * schematics well. Without it, the server's preview blocks are drawn as
 * translucent colored boxes by {@link GhostRenderer}. Both use the same
 * {@link Footprint}, so a schematic placed later covers what was previewed.
 */
public final class GhostPreview {
    /** What {@link GhostRenderer} draws: the mesh and the world position of its corner. */
    public record Ghost(GhostMesh mesh, int minX, int minY, int minZ) {}

    /** Where a schematic goes and how it is turned. */
    public record Placement(BlockPos origin, int quarterTurns) {}

    /** Ghost boxes are this opaque (of 255). */
    private static final int GHOST_ALPHA = 0x99;
    private static final int UNKNOWN_BLOCK_COLOR = 0x808080;
    /** Keeps ghost faces just off the faces of real blocks, so they do not flicker. */
    private static final float INFLATE = 0.002f;

    private final SchemGenSession session;
    private boolean busy;
    private Text message = Text.empty();

    /** Where the preview stands; {@code null} when there is none. */
    private BlockPos anchor;
    private int turns;
    /** The previewed schematic's footprint before turning. */
    private int sizeX;
    private int sizeZ;
    /** Ghost mode: the blocks and their colors, and the mesh for the current turn. */
    private PreviewGrid grid;
    private int[] colors;
    private Ghost ghost;
    /** Litematica mode: the preview schematic on disk. */
    private Path litematicaFile;

    public GhostPreview(SchemGenSession session) {
        this.session = session;
    }

    /** Preview the selected model with the form's settings where the player is looking. */
    public void request() {
        MinecraftClient client = MinecraftClient.getInstance();
        Path model = session.selectedModel();
        SettingsModel settings = session.settings();
        if (busy || model == null || settings == null) {
            return;
        }
        if (client.player == null) {
            message = Text.translatable("schemgen.preview.no_world");
            return;
        }
        BlockPos target = lookedAt(client);
        JsonObject request = settings.toPreviewJson(session.config().previewMaxSize);
        busy = true;
        message = Text.translatable("schemgen.preview.working");
        if (LitematicaSupport.isLoaded()) {
            Path folder = session.previewFolder();
            session.run(() -> {
                Conversions.Outcome outcome = Conversions.convert(session.backend().connect(), model, request, folder,
                        new Conversions.Listener() {
                            @Override
                            public void started(String jobId) {}

                            @Override
                            public void onUpdate(Job job) {}
                        });
                SchemGenSession.onClient(() -> showInLitematica(outcome, target));
            }, this::failed);
        } else {
            session.run(() -> {
                BackendClient backend = session.backend().connect();
                PreviewData data = backend.preview(model, request);
                Map<String, Integer> blockColors = backend.palette();
                SchemGenSession.onClient(() -> showGhost(data, blockColors, target));
            }, this::failed);
        }
    }

    /** Turn the preview a quarter turn clockwise, seen from above. */
    public void rotate() {
        if (anchor == null) {
            return;
        }
        turns = (turns + 1) % 4;
        placeAtAnchor();
    }

    /** Remove the preview from the world. */
    public void clear() {
        if (litematicaFile != null) {
            LitematicaSupport.clearPreview();
            deleteQuietly(litematicaFile);
            litematicaFile = null;
        }
        anchor = null;
        grid = null;
        colors = null;
        ghost = null;
        turns = 0;
        message = Text.empty();
    }

    /**
     * Where a schematic of {@code dims} should go to cover the preview's
     * spot, turned the same way — or {@code null} without a preview.
     */
    public Placement placementFor(int[] dims) {
        if (anchor == null || dims == null || dims.length != 3) {
            return null;
        }
        Footprint f = Footprint.centered(anchor.getX(), anchor.getY(), anchor.getZ(), dims[0], dims[2], turns);
        return new Placement(new BlockPos(f.originX(), f.originY(), f.originZ()), turns);
    }

    private void showGhost(PreviewData data, Map<String, Integer> blockColors, BlockPos target) {
        busy = false;
        clear();
        PreviewGrid decoded;
        try {
            decoded = data.grid();
        } catch (IllegalStateException e) {
            failed(e.getMessage());
            return;
        }
        int[] argb = new int[data.palette().size()];
        for (int i = 0; i < argb.length; i++) {
            argb[i] = GHOST_ALPHA << 24 | blockColors.getOrDefault(data.palette().get(i), UNKNOWN_BLOCK_COLOR);
        }
        grid = decoded;
        colors = argb;
        sizeX = decoded.sizeX();
        sizeZ = decoded.sizeZ();
        anchor = target;
        placeAtAnchor();
        message = Text.translatable("schemgen.preview.shown", decoded.count(), size(decoded.sizeX(), decoded.sizeY(), decoded.sizeZ()));
    }

    private void showInLitematica(Conversions.Outcome outcome, BlockPos target) {
        busy = false;
        clear();
        if (outcome.schematic() == null) {
            return; // cancelled
        }
        int[] dims = outcome.job().result().dims();
        sizeX = dims[0];
        sizeZ = dims[2];
        anchor = target;
        Placement placement = placementFor(dims);
        Text error = LitematicaSupport.showPreview(outcome.schematic(), placement.origin(), 0);
        if (error != null) {
            anchor = null;
            deleteQuietly(outcome.schematic());
            failed(error.getString());
            return;
        }
        litematicaFile = outcome.schematic();
        message = Text.translatable("schemgen.preview.litematica", size(dims[0], dims[1], dims[2]));
    }

    /** Put the preview on its anchor at the current turn. */
    private void placeAtAnchor() {
        Footprint f = Footprint.centered(anchor.getX(), anchor.getY(), anchor.getZ(), sizeX, sizeZ, turns);
        if (grid != null) {
            ghost = new Ghost(GhostMesh.of(grid.rotated(turns), colors, INFLATE), f.minX(), f.minY(), f.minZ());
        } else if (litematicaFile != null) {
            LitematicaSupport.movePreview(new BlockPos(f.originX(), f.originY(), f.originZ()), turns);
        }
    }

    private void failed(String reason) {
        busy = false;
        message = Text.translatable("schemgen.preview.failed", reason).formatted(Formatting.RED);
    }

    /** The block in front of the face the player looks at, else the player's own block. */
    private static BlockPos lookedAt(MinecraftClient client) {
        if (client.crosshairTarget instanceof BlockHitResult hit && hit.getType() == HitResult.Type.BLOCK) {
            return hit.getBlockPos().offset(hit.getSide());
        }
        return client.player.getBlockPos();
    }

    private static String size(int x, int y, int z) {
        return x + " × " + y + " × " + z;
    }

    private static void deleteQuietly(Path file) {
        try {
            Files.deleteIfExists(file);
        } catch (IOException e) {
            SchemGenClient.LOGGER.warn("Cannot delete {}", file, e);
        }
    }

    public boolean isBusy() {
        return busy;
    }

    public boolean isShown() {
        return anchor != null;
    }

    public Text message() {
        return message;
    }

    /** The ghost boxes to draw this frame; {@code null} when Litematica shows the preview or there is none. */
    public Ghost ghost() {
        return ghost;
    }
}
