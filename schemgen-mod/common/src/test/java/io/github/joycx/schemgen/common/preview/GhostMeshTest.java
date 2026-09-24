package io.github.joycx.schemgen.common.preview;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.junit.jupiter.api.Test;

class GhostMeshTest {
    private static final int RED = 0x80ff0000;

    @Test
    void aLoneBlockHasSixOutwardFaces() {
        GhostMesh mesh = GhostMesh.of(new PreviewGrid(1, 1, 1, new int[] {0, 0, 0, 0}), new int[] {RED}, 0);
        assertEquals(6, mesh.quads());
        for (int q = 0; q < 6; q++) {
            float[] normal = normal(mesh, q);
            float[] center = new float[3];
            for (int axis = 0; axis < 3; axis++) {
                for (int c = 0; c < 4; c++) {
                    center[axis] += mesh.corner(q, c, axis) / 4;
                }
            }
            // The face's normal points away from the cube's center (0.5, 0.5, 0.5).
            float outward = 0;
            for (int axis = 0; axis < 3; axis++) {
                outward += normal[axis] * (center[axis] - 0.5f);
            }
            assertTrue(outward > 0, "face " + q + " is wound counter-clockwise from outside");
        }
    }

    @Test
    void touchingFacesAreDroppedAndFacesAreShaded() {
        PreviewGrid bar = new PreviewGrid(2, 1, 1, new int[] {0, 0, 0, 0, 1, 0, 0, 0});
        GhostMesh mesh = GhostMesh.of(bar, new int[] {RED}, 0);
        assertEquals(10, mesh.quads());
        assertEquals(0x80800000, mesh.color(0), "bottom at half brightness, alpha kept");
        assertEquals(RED, mesh.color(1), "top at full brightness");
    }

    @Test
    void inflatedFacesSitJustOutsideTheBlock() {
        GhostMesh mesh = GhostMesh.of(new PreviewGrid(3, 3, 3, new int[] {2, 1, 0, 0}), new int[] {RED}, 0.01f);
        float min = Float.MAX_VALUE, max = -Float.MAX_VALUE;
        for (int q = 0; q < mesh.quads(); q++) {
            for (int c = 0; c < 4; c++) {
                min = Math.min(min, mesh.corner(q, c, 0));
                max = Math.max(max, mesh.corner(q, c, 0));
            }
        }
        assertEquals(1.99f, min, 1e-6);
        assertEquals(3.01f, max, 1e-6);
    }

    private static float[] normal(GhostMesh mesh, int q) {
        float[] a = new float[3], b = new float[3];
        for (int axis = 0; axis < 3; axis++) {
            a[axis] = mesh.corner(q, 1, axis) - mesh.corner(q, 0, axis);
            b[axis] = mesh.corner(q, 2, axis) - mesh.corner(q, 0, axis);
        }
        return new float[] {a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]};
    }
}
