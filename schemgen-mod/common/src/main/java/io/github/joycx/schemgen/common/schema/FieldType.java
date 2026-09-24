package io.github.joycx.schemgen.common.schema;

import com.google.gson.annotations.SerializedName;

/**
 * How a field's value is typed and edited. A type this build does not know
 * parses as {@code null}, and forms skip such a field instead of failing.
 */
public enum FieldType {
    /** Whole number; {@code min}/{@code max}/{@code step} apply. */
    @SerializedName("int")
    INT,
    /** Number; {@code min}/{@code max}/{@code step} apply. */
    @SerializedName("float")
    FLOAT,
    @SerializedName("bool")
    BOOL,
    @SerializedName("text")
    TEXT,
    /** A block ID such as {@code minecraft:stone}; {@code choices} are suggestions. */
    @SerializedName("block")
    BLOCK,
    /** A vector {@code [x, y, z]} in model space, Y up; edited as azimuth and elevation. */
    @SerializedName("direction")
    DIRECTION,
    /** One of {@code choices}. */
    @SerializedName("choice")
    CHOICE,
    /** A folder on the server's machine. The mod saves schematics itself and never shows these. */
    @SerializedName("folder")
    FOLDER
}
