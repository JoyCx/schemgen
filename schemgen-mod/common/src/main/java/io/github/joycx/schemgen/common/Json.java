package io.github.joycx.schemgen.common;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;

/** The Gson setups the mod reads and writes JSON with. */
public final class Json {
    /**
     * API bodies. Nulls are written: {@code "voxel_size": null} means "derive
     * it", which is not the same as leaving the server's default in place.
     */
    public static final Gson API = new GsonBuilder().serializeNulls().disableHtmlEscaping().create();

    /** Files people may open and edit, such as the config. */
    public static final Gson PRETTY = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create();

    private Json() {}
}
