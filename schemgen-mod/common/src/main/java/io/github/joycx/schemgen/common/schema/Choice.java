package io.github.joycx.schemgen.common.schema;

import com.google.gson.JsonElement;

/** One value a {@code choice} field can take, or a suggestion for a {@code block} field. */
public record Choice(JsonElement value, String label) {}
