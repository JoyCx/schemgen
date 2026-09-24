package io.github.joycx.schemgen.common.model;

import com.google.gson.annotations.SerializedName;
import java.util.List;

/** What a finished conversion produced. */
public record JobResult(
        int blocks,
        @SerializedName("unique_blocks") int uniqueBlocks,
        int[] dims,
        double seconds,
        List<Material> materials,
        String target,
        @SerializedName("data_version") int dataVersion) {}
