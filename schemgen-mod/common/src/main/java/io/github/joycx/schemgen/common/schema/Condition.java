package io.github.joycx.schemgen.common.schema;

import com.google.gson.JsonElement;

/** A field shows only while another field has this value. */
public record Condition(String field, JsonElement equals) {}
