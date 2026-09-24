package io.github.joycx.schemgen.common.preview;

/**
 * Where a schematic of {@code sizeX × sizeZ} blocks lands when it is turned by
 * quarter turns and centered on an anchor block.
 *
 * <p>{@code min*} is the corner of the turned footprint — where a ghost
 * preview draws {@link PreviewGrid#rotated} from. {@code origin*} is the
 * placement origin Litematica needs for the same footprint: it turns a
 * placement about its origin (clockwise: {@code (x, z) → (-z, x)}), so the
 * origin sits at a different corner for each turn. Using both, a preview and
 * the schematic placed later cover exactly the same blocks.
 */
public record Footprint(int minX, int minY, int minZ, int originX, int originY, int originZ) {
    public static Footprint centered(int anchorX, int anchorY, int anchorZ, int sizeX, int sizeZ, int quarterTurns) {
        int turns = Math.floorMod(quarterTurns, 4);
        boolean sideways = turns % 2 == 1;
        int turnedX = sideways ? sizeZ : sizeX;
        int turnedZ = sideways ? sizeX : sizeZ;
        int minX = anchorX - turnedX / 2;
        int minZ = anchorZ - turnedZ / 2;
        // The turned footprint's corner relative to the origin it was turned about.
        int cornerX = switch (turns) {
            case 1 -> -(sizeZ - 1);
            case 2 -> -(sizeX - 1);
            default -> 0;
        };
        int cornerZ = switch (turns) {
            case 2 -> -(sizeZ - 1);
            case 3 -> -(sizeX - 1);
            default -> 0;
        };
        return new Footprint(minX, anchorY, minZ, minX - cornerX, anchorY, minZ - cornerZ);
    }
}
