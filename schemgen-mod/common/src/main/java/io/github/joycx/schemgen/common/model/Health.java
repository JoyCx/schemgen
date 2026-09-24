package io.github.joycx.schemgen.common.model;

import com.google.gson.annotations.SerializedName;

/** {@code GET /api/health}: the one route that never needs the token. */
public record Health(
        String status,
        String name,
        String version,
        int api,
        String target,
        @SerializedName("data_version") int dataVersion,
        @SerializedName("schematic_version") int schematicVersion,
        String os,
        boolean auth) {

    public boolean ok() {
        return "ok".equals(status);
    }
}
