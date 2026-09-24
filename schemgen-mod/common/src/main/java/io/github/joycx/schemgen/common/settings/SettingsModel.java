package io.github.joycx.schemgen.common.settings;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;
import io.github.joycx.schemgen.common.schema.Choice;
import io.github.joycx.schemgen.common.schema.Condition;
import io.github.joycx.schemgen.common.schema.Field;
import io.github.joycx.schemgen.common.schema.FieldType;
import io.github.joycx.schemgen.common.schema.Schema;
import java.math.BigDecimal;
import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Set;

/**
 * The value of every setting the mod shows, starting from the server's
 * defaults. Setters hold values to what the schema allows — clamped to the
 * range, snapped to the step, whole for {@code int} — so the form, the
 * request and the remembered settings never disagree with the server.
 */
public final class SettingsModel {
    /**
     * Left out of the remembered settings: the target follows the game being
     * played, and a schematic name belongs to one model, not to the next.
     */
    private static final Set<String> NOT_REMEMBERED = Set.of("target", "schematic_name");

    /** The file format the mod asks for: Litematica's, since that is where the result goes. */
    public static final String FORMAT = "litematic";

    private final Schema schema;
    private final Map<String, JsonElement> values = new LinkedHashMap<>();

    public SettingsModel(Schema schema) {
        this.schema = schema;
        for (Field field : schema.fields()) {
            if (field.isUsedByMod()) {
                values.put(field.key(), field.defaultValue().deepCopy());
            }
        }
    }

    public Schema schema() {
        return schema;
    }

    /** A copy of the current value; JSON null for a nullable field left empty. */
    public JsonElement get(String key) {
        JsonElement value = values.get(key);
        return value == null ? JsonNull.INSTANCE : value.deepCopy();
    }

    public boolean isNull(String key) {
        JsonElement value = values.get(key);
        return value == null || value.isJsonNull();
    }

    /** The number held by a numeric field, or NaN when it is null. */
    public double number(String key) {
        JsonElement value = values.get(key);
        return value != null && value.isJsonPrimitive() && value.getAsJsonPrimitive().isNumber()
                ? value.getAsDouble()
                : Double.NaN;
    }

    public boolean bool(String key) {
        JsonElement value = values.get(key);
        return value != null && value.isJsonPrimitive() && value.getAsJsonPrimitive().isBoolean() && value.getAsBoolean();
    }

    /** A text, block, choice or number field's value as text; empty when null. */
    public String string(String key) {
        JsonElement value = values.get(key);
        return value != null && value.isJsonPrimitive() ? value.getAsString() : "";
    }

    public void reset(String key) {
        Field field = require(key);
        values.put(key, field.defaultValue().deepCopy());
    }

    /** Set a number, clamped to the field's range, snapped to its step and rounded for {@code int}. */
    public void setNumber(String key, double value) {
        Field field = require(key, FieldType.INT, FieldType.FLOAT);
        if (!Double.isFinite(value)) {
            throw new IllegalArgumentException(key + " must be a finite number");
        }
        double v = clamp(field, snap(field, value));
        values.put(key, field.type() == FieldType.INT ? new JsonPrimitive(Math.round(v)) : new JsonPrimitive(v));
    }

    /** Empty a nullable field — for {@code voxel_size}, "derive it from the max size". */
    public void setNull(String key) {
        Field field = require(key);
        if (!field.nullable()) {
            throw new IllegalArgumentException(key + " cannot be empty");
        }
        values.put(key, JsonNull.INSTANCE);
    }

    public void setBool(String key, boolean value) {
        require(key, FieldType.BOOL);
        values.put(key, new JsonPrimitive(value));
    }

    /**
     * Set a text, block or choice field. A choice may hold a value its list
     * does not name: the target can be a bare data version for a game newer
     * than the server's table.
     */
    public void setString(String key, String value) {
        require(key, FieldType.TEXT, FieldType.BLOCK, FieldType.CHOICE);
        values.put(key, new JsonPrimitive(value == null ? "" : value));
    }

    /** The direction a {@code direction} field holds, as given (not necessarily of length 1). */
    public double[] direction(String key) {
        require(key, FieldType.DIRECTION);
        double[] v = vector(values.get(key));
        return v == null ? new double[] {0, 1, 0} : v;
    }

    public double azimuth(String key) {
        return Directions.azimuth(direction(key));
    }

    public double elevation(String key) {
        return Directions.elevation(direction(key));
    }

    /**
     * Point a {@code direction} field along {@code (x, y, z)}, stored as a unit
     * vector — or as the server's exact default when it points the same way,
     * so an untouched light converts exactly like the CLI and the web UI.
     */
    public void setDirection(String key, double x, double y, double z) {
        Field field = require(key, FieldType.DIRECTION);
        double[] unit = Directions.normalize(new double[] {x, y, z});
        if (unit == null) {
            throw new IllegalArgumentException(key + " needs a direction, not a zero vector");
        }
        double[] fallback = Directions.normalize(defaultDirection(field));
        if (fallback != null && sameDirection(unit, fallback)) {
            values.put(key, field.defaultValue().deepCopy());
            return;
        }
        JsonArray array = new JsonArray();
        for (double c : unit) {
            array.add(c);
        }
        values.put(key, array);
    }

    /**
     * Point a {@code direction} field by angles in degrees. Angles on the
     * default's whole-degree position (where the sliders sit by default) mean
     * the default itself, as they do in the web UI.
     */
    public void setAngles(String key, double azimuthDeg, double elevationDeg) {
        Field field = require(key, FieldType.DIRECTION);
        double[] fallback = defaultDirection(field);
        if (fallback != null
                && Math.round(azimuthDeg) == Math.round(Directions.azimuth(fallback))
                && Math.round(elevationDeg) == Math.round(Directions.elevation(fallback))) {
            values.put(key, field.defaultValue().deepCopy());
            return;
        }
        double[] v = Directions.fromAngles(azimuthDeg, Math.max(-90, Math.min(90, elevationDeg)));
        setDirection(key, v[0], v[1], v[2]);
    }

    /** Whether a field shows: always, or while the field its {@code when} names has the given value. */
    public boolean isVisible(Field field) {
        Condition when = field.when();
        if (when == null) {
            return true;
        }
        JsonElement current = values.get(when.field());
        JsonElement expected = when.equals() == null ? JsonNull.INSTANCE : when.equals();
        return current != null && current.equals(expected);
    }

    /** The {@code settings} object for {@code POST /api/jobs}: every field the mod uses. */
    public JsonObject toJson() {
        JsonObject out = new JsonObject();
        values.forEach((key, value) -> out.add(key, value.deepCopy()));
        if (schema.field("format").isPresent()) {
            out.addProperty("format", FORMAT);
        }
        return out;
    }

    /**
     * The settings for a preview: at most {@code maxSize} blocks along the
     * longest side, and without an explicit voxel size, which would override
     * that cap. A preview is for looking, and a small one comes back quickly.
     */
    public JsonObject toPreviewJson(int maxSize) {
        JsonObject out = toJson();
        double current = number("max_size");
        if (!Double.isNaN(current)) {
            out.addProperty("max_size", Math.min(Math.round(current), maxSize));
        }
        if (out.has("voxel_size")) {
            out.add("voxel_size", JsonNull.INSTANCE);
        }
        return out;
    }

    /**
     * The settings worth remembering between sessions: those the player
     * changed. Untouched ones are left out, so they keep following the
     * server's defaults — also when a newer server changes them.
     */
    public JsonObject snapshot() {
        JsonObject out = new JsonObject();
        values.forEach((key, value) -> {
            Field field = schema.field(key).orElseThrow();
            if (!NOT_REMEMBERED.contains(key) && !value.equals(field.defaultValue())) {
                out.add(key, value.deepCopy());
            }
        });
        return out;
    }

    /**
     * Apply remembered settings. Unknown keys and values that no longer fit
     * (the server changed a field) are skipped and keep their defaults.
     */
    public void restore(JsonObject saved) {
        for (Map.Entry<String, JsonElement> entry : saved.entrySet()) {
            Field field = schema.field(entry.getKey()).orElse(null);
            if (field == null || !values.containsKey(field.key()) || NOT_REMEMBERED.contains(field.key())) {
                continue;
            }
            try {
                apply(field, entry.getValue());
            } catch (RuntimeException stale) {
                reset(field.key());
            }
        }
    }

    private void apply(Field field, JsonElement value) {
        String key = field.key();
        if (value.isJsonNull()) {
            setNull(key);
            return;
        }
        switch (field.type()) {
            case INT, FLOAT -> setNumber(key, primitive(value).getAsDouble());
            case BOOL -> {
                JsonPrimitive p = primitive(value);
                if (!p.isBoolean()) {
                    throw new IllegalArgumentException("not a boolean");
                }
                setBool(key, p.getAsBoolean());
            }
            case TEXT, BLOCK -> setString(key, primitive(value).getAsString());
            case CHOICE -> {
                boolean listed = field.choices().stream().map(Choice::value).anyMatch(value::equals);
                if (!listed) {
                    throw new IllegalArgumentException("not one of the choices");
                }
                values.put(key, value.deepCopy());
            }
            case DIRECTION -> {
                double[] v = vector(value);
                if (v == null) {
                    throw new IllegalArgumentException("not a direction");
                }
                setDirection(key, v[0], v[1], v[2]);
            }
            default -> throw new IllegalArgumentException("not remembered");
        }
    }

    private Field require(String key, FieldType... types) {
        Field field = schema.field(key)
                .filter(f -> values.containsKey(f.key()))
                .orElseThrow(() -> new IllegalArgumentException("No setting " + key));
        if (types.length > 0 && Arrays.stream(types).noneMatch(t -> t == field.type())) {
            throw new IllegalArgumentException(key + " is a " + field.type() + " setting");
        }
        return field;
    }

    private static JsonPrimitive primitive(JsonElement value) {
        if (!value.isJsonPrimitive()) {
            throw new IllegalArgumentException("not a single value");
        }
        return value.getAsJsonPrimitive();
    }

    /** Snap to the step grid, anchored at the minimum like an HTML range input. */
    private static double snap(Field field, double value) {
        Double step = field.step();
        if (step == null || !(step > 0)) {
            return value;
        }
        double base = field.min() == null ? 0 : field.min();
        // BigDecimal keeps 0.35 from coming back as 0.35000000000000003.
        BigDecimal steps = BigDecimal.valueOf(Math.round((value - base) / step));
        return BigDecimal.valueOf(base).add(steps.multiply(BigDecimal.valueOf(step))).doubleValue();
    }

    private static double clamp(Field field, double value) {
        if (field.min() != null) {
            value = Math.max(value, field.min());
        }
        if (field.max() != null) {
            value = Math.min(value, field.max());
        }
        return value;
    }

    private static double[] defaultDirection(Field field) {
        return vector(field.defaultValue());
    }

    private static double[] vector(JsonElement value) {
        if (value == null || !value.isJsonArray() || value.getAsJsonArray().size() != 3) {
            return null;
        }
        double[] v = new double[3];
        for (int i = 0; i < 3; i++) {
            JsonElement c = value.getAsJsonArray().get(i);
            if (!c.isJsonPrimitive() || !c.getAsJsonPrimitive().isNumber()) {
                return null;
            }
            v[i] = c.getAsDouble();
        }
        return v;
    }

    /**
     * Whether two unit vectors point the same way, to within the precision of
     * the server's 32-bit floats: its default {@code (0.35, 0.85, 0.40)}
     * arrives as {@code 0.3499999940395355, …}.
     */
    private static boolean sameDirection(double[] a, double[] b) {
        for (int i = 0; i < 3; i++) {
            if (Math.abs(a[i] - b[i]) > 1e-6) {
                return false;
            }
        }
        return true;
    }
}
