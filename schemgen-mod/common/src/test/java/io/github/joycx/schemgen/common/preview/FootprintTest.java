package io.github.joycx.schemgen.common.preview;

import static org.junit.jupiter.api.Assertions.assertEquals;

import java.util.HashSet;
import java.util.List;
import java.util.Set;
import org.junit.jupiter.api.Test;

class FootprintTest {
    /** Litematica's PositionUtils.getTransformedBlockPos for a rotation (no mirror). */
    private static int[] litematicaTurn(int x, int z, int turns) {
        return switch (turns) {
            case 1 -> new int[] {-z, x};
            case 2 -> new int[] {-x, -z};
            case 3 -> new int[] {z, -x};
            default -> new int[] {x, z};
        };
    }

    @Test
    void theGhostAndTheLitematicaPlacementCoverTheSameBlocks() {
        int sizeX = 5, sizeZ = 3;
        int[] blocks = new int[sizeX * sizeZ * 4];
        int i = 0;
        for (int x = 0; x < sizeX; x++) {
            for (int z = 0; z < sizeZ; z++) {
                blocks[i++] = x;
                blocks[i++] = 0;
                blocks[i++] = z;
                blocks[i++] = 0;
            }
        }
        PreviewGrid grid = new PreviewGrid(sizeX, 1, sizeZ, blocks);
        for (int turns = -1; turns <= 4; turns++) {
            Footprint f = Footprint.centered(100, 64, -40, sizeX, sizeZ, turns);
            PreviewGrid turned = grid.rotated(turns);
            Set<List<Integer>> ghost = new HashSet<>();
            Set<List<Integer>> placed = new HashSet<>();
            for (int b = 0; b < grid.count(); b++) {
                ghost.add(List.of(f.minX() + turned.x(b), f.minZ() + turned.z(b)));
                int[] t = litematicaTurn(grid.x(b), grid.z(b), Math.floorMod(turns, 4));
                placed.add(List.of(f.originX() + t[0], f.originZ() + t[1]));
            }
            assertEquals(ghost, placed, "turns " + turns);
        }
    }

    @Test
    void theFootprintIsCenteredOnTheAnchor() {
        Footprint straight = Footprint.centered(10, 70, 20, 4, 6, 0);
        assertEquals(new Footprint(8, 70, 17, 8, 70, 17), straight);
        Footprint turned = Footprint.centered(10, 70, 20, 4, 6, 1);
        assertEquals(7, turned.minX(), "6 wide once turned");
        assertEquals(18, turned.minZ());
    }
}
