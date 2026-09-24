package io.github.joycx.schemgen.common.schema;

import com.google.gson.annotations.SerializedName;

/** A Minecraft version the server can write schematics for. */
public record Target(
        String id,
        @SerializedName("data_version") int dataVersion,
        @SerializedName("schematic_version") int schematicVersion) {}
