package io.github.joycx.schemgen.common.schema;

import com.google.gson.JsonParseException;
import com.google.gson.annotations.SerializedName;
import io.github.joycx.schemgen.common.Json;
import java.util.List;
import java.util.Objects;
import java.util.Optional;

/**
 * {@code GET /api/schema}: every setting with its type, range, default, group,
 * label and help; the Minecraft versions the server can target; its formats,
 * palette size and upload limit.
 */
public record Schema(
        String version,
        List<Group> groups,
        List<Field> fields,
        List<Target> targets,
        @SerializedName("default_target") String defaultTarget,
        List<Format> formats,
        PaletteSummary palette,
        Limits limits) {

    public record PaletteSummary(int entries, int blocks) {}

    public record Limits(@SerializedName("max_upload_bytes") long maxUploadBytes) {}

    public Schema {
        groups = listOf(groups);
        fields = listOf(fields);
        targets = listOf(targets);
        formats = listOf(formats);
    }

    public static Schema parse(String json) {
        Schema schema = Json.API.fromJson(json, Schema.class);
        if (schema == null || schema.fields.isEmpty()) {
            throw new JsonParseException("Not a settings schema: no fields");
        }
        return schema;
    }

    public Optional<Field> field(String key) {
        return fields.stream().filter(f -> f.key().equals(key)).findFirst();
    }

    /** The fields of one group the mod shows, in display order (see {@link Field#isUsedByMod()}). */
    public List<Field> fieldsIn(Group group) {
        return fields.stream()
                .filter(f -> f.isUsedByMod() && Objects.equals(f.group(), group.key()))
                .toList();
    }

    private static <T> List<T> listOf(List<T> list) {
        return list == null ? List.of() : List.copyOf(list);
    }
}
