package io.github.joycx.schemgen.common.preview;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.JsonParseException;
import io.github.joycx.schemgen.common.model.Material;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.Base64;
import java.util.List;
import org.junit.jupiter.api.Test;

class PreviewDataTest {
    /** Little-endian int32 quadruples, base64'd by hand — what the server's {@code pack} produces. */
    private static String pack(int... ints) {
        ByteBuffer bytes = ByteBuffer.allocate(ints.length * 4).order(ByteOrder.LITTLE_ENDIAN);
        for (int i : ints) {
            bytes.putInt(i);
        }
        return Base64.getEncoder().encodeToString(bytes.array());
    }

    private static PreviewData preview(String blocks, int count) {
        return PreviewData.parse("{\"dims\":[48,34,300],\"origin\":[-1.0,-1.0,-1.0],\"pitch\":0.0583,"
                + "\"palette\":[\"minecraft:blue_concrete\",\"minecraft:yellow_terracotta\"],\"count\":" + count
                + ",\"blocks\":\"" + blocks + "\",\"materials\":[{\"name\":\"minecraft:blue_concrete\",\"count\":1}],"
                + "\"target\":\"1.21.8\",\"capped\":true,\"seconds\":0.8}");
    }

    @Test
    void decodesLittleEndianQuadruples() {
        PreviewData data = preview(pack(1, 2, 3, 1, 0, 0, 258, 0), 2);
        assertArrayEquals(new int[] {1, 2, 3, 1, 0, 0, 258, 0}, data.decodeBlocks());
        assertEquals(List.of(new Material("minecraft:blue_concrete", 1)), data.materials());
        assertArrayEquals(new double[] {-1, -1, -1}, data.origin());
        assertTrue(data.capped());
        assertEquals(2, data.grid().count());
        assertEquals(258, data.grid().z(1));
    }

    @Test
    void theServersOwnExampleDecodes() {
        // The bytes of backend/crates/server/src/preview.rs `packs_little_endian_quadruples`.
        String packed = Base64.getEncoder().encodeToString(new byte[] {
            1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 0, 0, 0, 0, 0, 0});
        assertArrayEquals(new int[] {1, 2, 3, 1, 0, 0, 258, 0}, preview(packed, 2).decodeBlocks());
    }

    @Test
    void anEmptyPreviewDecodesToNothing() {
        assertArrayEquals(new int[0], preview("", 0).decodeBlocks());
    }

    @Test
    void inconsistentBlocksAreRejected() {
        assertThrows(IllegalStateException.class, () -> preview(pack(1, 2, 3, 1), 2).decodeBlocks(), "count");
        assertThrows(IllegalStateException.class, () -> preview(pack(1, 2, 3, 2), 1).decodeBlocks(), "palette index");
        assertThrows(IllegalStateException.class, () -> preview(pack(48, 2, 3, 0), 1).decodeBlocks(), "outside dims");
        assertThrows(IllegalStateException.class, () -> preview(pack(-1, 2, 3, 0), 1).decodeBlocks(), "negative");
        String notQuadruples = Base64.getEncoder().encodeToString(new byte[12]);
        assertThrows(IllegalStateException.class, () -> preview(notQuadruples, 1).decodeBlocks());
        assertThrows(JsonParseException.class, () -> PreviewData.parse("{\"error\":\"x\"}"));
    }
}
