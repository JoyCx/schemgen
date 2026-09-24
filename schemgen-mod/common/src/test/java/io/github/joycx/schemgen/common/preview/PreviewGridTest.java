package io.github.joycx.schemgen.common.preview;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

class PreviewGridTest {
    private static final int ALL = PreviewGrid.DOWN | PreviewGrid.UP | PreviewGrid.NORTH
            | PreviewGrid.SOUTH | PreviewGrid.WEST | PreviewGrid.EAST;

    @Test
    void aQuarterTurnIsClockwiseSeenFromAbove() {
        // 3 wide (x), 1 high, 2 deep (z); one block in the north-east corner.
        PreviewGrid grid = new PreviewGrid(3, 1, 2, new int[] {2, 0, 0, 7});
        PreviewGrid turned = grid.rotated(1);

        assertEquals(2, turned.sizeX());
        assertEquals(3, turned.sizeZ());
        // North-east goes to south-east: east turns into south, north into east.
        assertEquals(1, turned.x(0));
        assertEquals(2, turned.z(0));
        assertEquals(7, turned.paletteIndex(0));
    }

    @Test
    void fourQuarterTurnsAreNone() {
        PreviewGrid grid = new PreviewGrid(3, 2, 4, new int[] {0, 0, 0, 0, 2, 1, 3, 1, 1, 0, 2, 0});
        PreviewGrid back = grid.rotated(4);
        PreviewGrid alsoBack = grid.rotated(-1).rotated(1);
        for (int b = 0; b < grid.count(); b++) {
            assertEquals(grid.x(b), back.x(b));
            assertEquals(grid.z(b), back.z(b));
            assertEquals(grid.x(b), alsoBack.x(b));
            assertEquals(grid.z(b), alsoBack.z(b));
        }
        assertEquals(grid.rotated(2).x(1), grid.rotated(1).rotated(1).x(1));
    }

    @Test
    void onlyFacesWithoutANeighbourAreExposed() {
        PreviewGrid bar = new PreviewGrid(3, 3, 3, new int[] {0, 0, 0, 0, 1, 0, 0, 0});
        int[] faces = bar.exposedFaces();
        assertEquals(ALL & ~PreviewGrid.EAST, faces[0]);
        assertEquals(ALL & ~PreviewGrid.WEST, faces[1]);

        PreviewGrid tower = new PreviewGrid(1, 3, 1, new int[] {0, 0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0});
        assertEquals(ALL & ~PreviewGrid.UP, tower.exposedFaces()[0]);
        assertEquals(ALL & ~PreviewGrid.UP & ~PreviewGrid.DOWN, tower.exposedFaces()[1]);
        assertEquals(ALL & ~PreviewGrid.DOWN, tower.exposedFaces()[2]);
    }
}
