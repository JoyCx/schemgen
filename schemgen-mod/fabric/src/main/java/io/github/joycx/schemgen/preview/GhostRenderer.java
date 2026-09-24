package io.github.joycx.schemgen.preview;

import io.github.joycx.schemgen.common.preview.GhostMesh;
import net.minecraft.client.render.RenderLayer;
import net.minecraft.client.render.VertexConsumer;
import net.minecraft.client.render.VertexConsumerProvider;
import net.minecraft.client.util.math.MatrixStack;
import net.minecraft.util.math.Vec3d;
//? if >=1.21.10 {
/*import net.fabricmc.fabric.api.client.rendering.v1.world.WorldRenderEvents;
*///?} else {
import net.fabricmc.fabric.api.client.rendering.v1.WorldRenderEvents;
//?}
//? if >=1.21.11 {
/*import net.minecraft.client.render.RenderLayers;
*///?}

/**
 * Draws the ghost preview — translucent colored boxes where the blocks would
 * go — when Litematica is not installed to show a real placement. World
 * rendering is the part of Minecraft's API that moves most between versions,
 * so this class holds all of it and nothing else.
 *
 * <p>The boxes go into the world renderer's own vertex consumers after
 * entities: Fabric API documents that point, in every version here, as one
 * where they are available and drawn by the renderer, camera-relative.
 */
public final class GhostRenderer {
    private GhostRenderer() {}

    public static void register(GhostPreview preview) {
        //? if >=1.21.10 {
        /*WorldRenderEvents.AFTER_ENTITIES.register(context -> draw(preview.ghost(), context.matrices(),
                context.consumers(), context.worldState().cameraRenderState.pos));
        *///?} else {
        WorldRenderEvents.AFTER_ENTITIES.register(context -> draw(preview.ghost(), context.matrixStack(),
                context.consumers(), context.camera().getPos()));
        //?}
    }

    private static void draw(GhostPreview.Ghost ghost, MatrixStack matrices, VertexConsumerProvider consumers,
            Vec3d camera) {
        if (ghost == null || matrices == null || consumers == null) {
            return;
        }
        VertexConsumer buffer = consumers.getBuffer(layer());
        GhostMesh mesh = ghost.mesh();
        matrices.push();
        matrices.translate(ghost.minX() - camera.x, ghost.minY() - camera.y, ghost.minZ() - camera.z);
        MatrixStack.Entry entry = matrices.peek();
        for (int q = 0; q < mesh.quads(); q++) {
            int argb = mesh.color(q);
            int r = argb >> 16 & 0xff;
            int g = argb >> 8 & 0xff;
            int b = argb & 0xff;
            int a = argb >>> 24;
            for (int corner = 0; corner < 4; corner++) {
                buffer.vertex(entry, mesh.corner(q, corner, 0), mesh.corner(q, corner, 1), mesh.corner(q, corner, 2))
                        .color(r, g, b, a);
            }
        }
        matrices.pop();
    }

    /** Translucent, untextured quads: the layer Minecraft's own debug overlays use. */
    private static RenderLayer layer() {
        //? if >=1.21.11 {
        /*return RenderLayers.debugQuads();
        *///?} else {
        return RenderLayer.getDebugQuads();
        //?}
    }
}
