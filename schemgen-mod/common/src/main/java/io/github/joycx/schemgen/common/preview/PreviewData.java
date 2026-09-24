package io.github.joycx.schemgen.common.preview;

import com.google.gson.JsonParseException;
import io.github.joycx.schemgen.common.Json;
import io.github.joycx.schemgen.common.model.Material;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.IntBuffer;
import java.util.Base64;
import java.util.List;

/**
 * {@code POST /api/preview}: the blocks a conversion would produce, without a
 * file. {@code blocks} is base64 of little-endian 32-bit integers, four per
 * block: {@code x, y, z, palette index}.
 */
public record PreviewData(
        int[] dims,
        double[] origin,
        double pitch,
        List<String> palette,
        int count,
        String blocks,
        List<Material> materials,
        String target,
        boolean capped,
        double seconds) {

    public PreviewData {
        palette = palette == null ? List.of() : List.copyOf(palette);
        materials = materials == null ? List.of() : List.copyOf(materials);
    }

    public static PreviewData parse(String json) {
        PreviewData data = Json.API.fromJson(json, PreviewData.class);
        if (data == null || data.dims == null || data.dims.length != 3) {
            throw new JsonParseException("Not a preview: no dims");
        }
        return data;
    }

    /**
     * The packed blocks as {@code [x0, y0, z0, i0, x1, …]}, checked against
     * {@code count}, {@code dims} and the palette so a renderer can index
     * without bounds checks of its own.
     */
    public int[] decodeBlocks() {
        byte[] bytes = Base64.getDecoder().decode(blocks == null ? "" : blocks);
        if (bytes.length % 16 != 0) {
            throw new IllegalStateException("Preview blocks are not whole x,y,z,index quadruples");
        }
        IntBuffer ints = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN).asIntBuffer();
        int[] out = new int[ints.remaining()];
        ints.get(out);
        if (out.length / 4 != count) {
            throw new IllegalStateException("Preview says " + count + " blocks but holds " + out.length / 4);
        }
        for (int i = 0; i < out.length; i += 4) {
            for (int axis = 0; axis < 3; axis++) {
                if (out[i + axis] < 0 || out[i + axis] >= dims[axis]) {
                    throw new IllegalStateException("Preview block outside its " + dims[0] + "×" + dims[1] + "×" + dims[2] + " grid");
                }
            }
            if (out[i + 3] < 0 || out[i + 3] >= palette.size()) {
                throw new IllegalStateException("Preview block uses palette entry " + out[i + 3] + " of " + palette.size());
            }
        }
        return out;
    }

    public PreviewGrid grid() {
        return new PreviewGrid(dims[0], dims[1], dims[2], decodeBlocks());
    }
}
