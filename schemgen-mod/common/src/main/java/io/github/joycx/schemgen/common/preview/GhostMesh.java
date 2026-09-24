package io.github.joycx.schemgen.common.preview;

/**
 * The translucent boxes of a ghost preview as quads, computed once when the
 * preview changes so the renderer only copies vertices each frame. Only faces
 * that border no other block are kept, each wound counter-clockwise seen from
 * outside and shaded like Minecraft's own blocks (tops brightest, bottoms
 * darkest) so the shape reads without textures.
 */
public final class GhostMesh {
    /** Corners of a unit cube's faces in {@link PreviewGrid} face order, outward counter-clockwise. */
    private static final float[][] FACES = {
        {0, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1}, // down
        {0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0}, // up
        {0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0}, // north
        {0, 0, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1}, // south
        {0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0}, // west
        {1, 0, 0, 1, 1, 0, 1, 1, 1, 1, 0, 1}, // east
    };
    private static final float[] SHADE = {0.5f, 1.0f, 0.8f, 0.8f, 0.6f, 0.6f};

    private final float[] corners;
    private final int[] colors;

    private GhostMesh(float[] corners, int[] colors) {
        this.corners = corners;
        this.colors = colors;
    }

    /**
     * @param argb color per palette index; the alpha is the ghost's opacity
     * @param inflate how far faces sit outside their block, so a ghost drawn
     *     where a real block stands does not flicker against it
     */
    public static GhostMesh of(PreviewGrid grid, int[] argb, float inflate) {
        int[] exposed = grid.exposedFaces();
        int quads = 0;
        for (int mask : exposed) {
            quads += Integer.bitCount(mask);
        }
        float[] corners = new float[quads * 12];
        int[] colors = new int[quads];
        int q = 0;
        for (int b = 0; b < grid.count(); b++) {
            int color = argb[grid.paletteIndex(b)];
            for (int face = 0; face < 6; face++) {
                if ((exposed[b] & (1 << face)) == 0) {
                    continue;
                }
                float[] unit = FACES[face];
                for (int c = 0; c < 12; c += 3) {
                    corners[q * 12 + c] = grid.x(b) + expand(unit[c], inflate);
                    corners[q * 12 + c + 1] = grid.y(b) + expand(unit[c + 1], inflate);
                    corners[q * 12 + c + 2] = grid.z(b) + expand(unit[c + 2], inflate);
                }
                colors[q++] = shade(color, SHADE[face]);
            }
        }
        return new GhostMesh(corners, colors);
    }

    public int quads() {
        return colors.length;
    }

    /** Corner {@code 0..3} of quad {@code q}, axis {@code 0..2} (x, y, z), relative to the grid's corner. */
    public float corner(int q, int corner, int axis) {
        return corners[q * 12 + corner * 3 + axis];
    }

    /** ARGB color of quad {@code q}. */
    public int color(int q) {
        return colors[q];
    }

    private static float expand(float unit, float inflate) {
        return unit == 0 ? -inflate : 1 + inflate;
    }

    private static int shade(int argb, float factor) {
        int r = Math.round(((argb >> 16) & 0xff) * factor);
        int g = Math.round(((argb >> 8) & 0xff) * factor);
        int b = Math.round((argb & 0xff) * factor);
        return (argb & 0xff000000) | r << 16 | g << 8 | b;
    }
}
