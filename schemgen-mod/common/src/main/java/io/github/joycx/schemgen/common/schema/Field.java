package io.github.joycx.schemgen.common.schema;

import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.annotations.SerializedName;
import java.util.List;

/**
 * One setting as {@code GET /api/schema} describes it. Forms are rendered from
 * these, so the mod never hard-codes a setting, its range or its default.
 */
public record Field(
        String key,
        FieldType type,
        String group,
        String label,
        String help,
        @SerializedName("default") JsonElement defaultValue,
        Scope scope,
        Double min,
        Double max,
        Double step,
        double[] slider,
        String unit,
        String placeholder,
        List<Choice> choices,
        boolean nullable,
        boolean advanced,
        Condition when) {

    public Field {
        choices = choices == null ? List.of() : List.copyOf(choices);
        // A missing default and an explicit null both mean "null".
        defaultValue = defaultValue == null ? JsonNull.INSTANCE : defaultValue;
    }

    public boolean isNumber() {
        return type == FieldType.INT || type == FieldType.FLOAT;
    }

    /**
     * The range a slider should span — {@code slider} when the schema narrows
     * it, else {@code min}…{@code max} — or {@code null} when the field has an
     * open end and needs a text box instead.
     */
    public double[] sliderRange() {
        if (slider != null && slider.length == 2 && slider[0] < slider[1]) {
            return slider.clone();
        }
        if (min != null && max != null && min < max) {
            return new double[] {min, max};
        }
        return null;
    }

    /**
     * Whether the mod shows and sends this field. Job-scope fields
     * ({@code output_dir}, {@code threads}) only matter to batches and to the
     * server's own copy of the file; the mod converts one model at a time and
     * saves the download itself. {@code format} is not the player's to pick:
     * the mod places what it makes in Litematica, so it always asks for a
     * {@code .litematic}. Types this build does not know are skipped.
     */
    public boolean isUsedByMod() {
        return type != null && type != FieldType.FOLDER && scope != Scope.JOB && !"format".equals(key);
    }
}
