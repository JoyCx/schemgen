package io.github.joycx.schemgen.common.schema;

import com.google.gson.annotations.SerializedName;

/** What a field affects. */
public enum Scope {
    /** Changes the blocks a conversion produces. */
    @SerializedName("conversion")
    CONVERSION,
    /** Changes only how a job runs or where its file goes ({@code output_dir}, {@code threads}). */
    @SerializedName("job")
    JOB
}
