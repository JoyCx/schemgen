package io.github.joycx.schemgen.common.preview;

import java.util.BitSet;

/**
 * Preview blocks on their grid: {@code [x, y, z, palette index]} per block,
 * all coordinates in {@code 0 ≤ c < size}.
 *
 * <p>Minecraft's axes: x east, y up, z south.
 */
public final class PreviewGrid {
    /** Face bits in Minecraft's {@code Direction} order: down, up, north, south, west, east. */
    public static final int DOWN = 1, UP = 1 << 1, NORTH = 1 << 2, SOUTH = 1 << 3, WEST = 1 << 4, EAST = 1 << 5;

    private final int sizeX;
    private final int sizeY;
    private final int sizeZ;
    private final int[] blocks;

    public PreviewGrid(int sizeX, int sizeY, int sizeZ, int[] blocks) {
        if (blocks.length % 4 != 0) {
            throw new IllegalArgumentException("blocks must be x, y, z, index quadruples");
        }
        // Previews are at most 256 on a side; the cell index must fit an int.
        if ((long) sizeX * sizeY * sizeZ > Integer.MAX_VALUE) {
            throw new IllegalArgumentException("grid too large for a preview");
        }
        this.sizeX = sizeX;
        this.sizeY = sizeY;
        this.sizeZ = sizeZ;
        this.blocks = blocks;
    }

    public int sizeX() {
        return sizeX;
    }

    public int sizeY() {
        return sizeY;
    }

    public int sizeZ() {
        return sizeZ;
    }

    public int count() {
        return blocks.length / 4;
    }

    public int x(int block) {
        return blocks[block * 4];
    }

    public int y(int block) {
        return blocks[block * 4 + 1];
    }

    public int z(int block) {
        return blocks[block * 4 + 2];
    }

    public int paletteIndex(int block) {
        return blocks[block * 4 + 3];
    }

    /**
     * This grid turned {@code quarterTurns} times clockwise seen from above —
     * what Minecraft's {@code CLOCKWISE_90} does: east becomes south — and moved
     * back so its corner stays at the origin.
     */
    public PreviewGrid rotated(int quarterTurns) {
        int turns = Math.floorMod(quarterTurns, 4);
        PreviewGrid grid = this;
        for (int i = 0; i < turns; i++) {
            grid = grid.quarterTurn();
        }
        return grid;
    }

    private PreviewGrid quarterTurn() {
        // (x, z) → (-z, x), shifted by sizeZ - 1 to stay non-negative.
        int[] out = new int[blocks.length];
        for (int i = 0; i < blocks.length; i += 4) {
            out[i] = sizeZ - 1 - blocks[i + 2];
            out[i + 1] = blocks[i + 1];
            out[i + 2] = blocks[i];
            out[i + 3] = blocks[i + 3];
        }
        return new PreviewGrid(sizeZ, sizeY, sizeX, out);
    }

    /**
     * For each block, the faces that border no other block of the grid — the
     * only ones a renderer needs to draw. A solid preview draws a fraction of
     * its faces this way.
     */
    public int[] exposedFaces() {
        BitSet occupied = new BitSet();
        for (int b = 0; b < count(); b++) {
            occupied.set(cell(x(b), y(b), z(b)));
        }
        int[] faces = new int[count()];
        for (int b = 0; b < count(); b++) {
            int x = x(b), y = y(b), z = z(b);
            int mask = 0;
            if (!has(occupied, x, y - 1, z)) mask |= DOWN;
            if (!has(occupied, x, y + 1, z)) mask |= UP;
            if (!has(occupied, x, y, z - 1)) mask |= NORTH;
            if (!has(occupied, x, y, z + 1)) mask |= SOUTH;
            if (!has(occupied, x - 1, y, z)) mask |= WEST;
            if (!has(occupied, x + 1, y, z)) mask |= EAST;
            faces[b] = mask;
        }
        return faces;
    }

    private boolean has(BitSet occupied, int x, int y, int z) {
        return x >= 0 && y >= 0 && z >= 0 && x < sizeX && y < sizeY && z < sizeZ && occupied.get(cell(x, y, z));
    }

    private int cell(int x, int y, int z) {
        return (y * sizeZ + z) * sizeX + x;
    }
}
