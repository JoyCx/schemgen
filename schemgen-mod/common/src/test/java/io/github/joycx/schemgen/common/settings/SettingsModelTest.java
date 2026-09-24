package io.github.joycx.schemgen.common.settings;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.JsonArray;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import io.github.joycx.schemgen.common.Fixtures;
import io.github.joycx.schemgen.common.schema.Field;
import io.github.joycx.schemgen.common.schema.Schema;
import org.junit.jupiter.api.Test;

class SettingsModelTest {
    private final Schema schema = Fixtures.schema();
    private final SettingsModel model = new SettingsModel(schema);

    private Field field(String key) {
        return schema.field(key).orElseThrow();
    }

    @Test
    void startsFromTheServersDefaults() {
        assertEquals(128, model.number("max_size"));
        assertTrue(model.bool("dither"));
        assertTrue(model.isNull("voxel_size"));
        assertEquals("1.21.8", model.string("target"));
        assertEquals("minecraft:white_concrete", model.string("default_block"));
        assertEquals(field("light_dir").defaultValue(), model.get("light_dir"));
    }

    @Test
    void numbersAreClampedSnappedAndRounded() {
        model.setNumber("max_size", 5000);
        assertEquals(2048, model.number("max_size"));
        model.setNumber("max_size", 0);
        assertEquals(1, model.number("max_size"));
        model.setNumber("max_size", 64.4);
        assertEquals("64", model.get("max_size").toString(), "an int goes out as an integer");

        model.setNumber("brightness", 0.3456);
        assertEquals(0.35, model.number("brightness"));
        model.setNumber("brightness", -7);
        assertEquals(-1.0, model.number("brightness"));
        model.setNumber("ram_limit", 1.3);
        assertEquals(1.5, model.number("ram_limit"), "step 0.5 from the minimum 0.5");
        model.setNumber("voxel_size", 0.25);
        assertEquals(0.25, model.number("voxel_size"), "no step, no maximum");
        model.setNull("voxel_size");
        assertTrue(model.isNull("voxel_size"));
    }

    @Test
    void settersRejectWhatTheFieldCannotHold() {
        assertThrows(IllegalArgumentException.class, () -> model.setNumber("max_size", Double.NaN));
        assertThrows(IllegalArgumentException.class, () -> model.setNumber("dither", 1));
        assertThrows(IllegalArgumentException.class, () -> model.setBool("max_size", true));
        assertThrows(IllegalArgumentException.class, () -> model.setNull("max_size"));
        assertThrows(IllegalArgumentException.class, () -> model.setString("output_dir", "/tmp"),
                "folders are not the mod's");
        assertThrows(IllegalArgumentException.class, () -> model.setNumber("nope", 1));
    }

    @Test
    void visibilityFollowsWhen() {
        assertTrue(model.isVisible(field("dither")));
        assertFalse(model.isVisible(field("default_block")));
        model.setBool("color_sampling", false);
        assertFalse(model.isVisible(field("dither")));
        assertFalse(model.isVisible(field("light_dir")));
        assertTrue(model.isVisible(field("default_block")));
        assertTrue(model.isVisible(field("max_size")), "no condition");
    }

    @Test
    void jsonHasEverySettingTheModSendsAndNothingElse() {
        JsonObject json = model.toJson();
        assertFalse(json.has("output_dir"));
        assertFalse(json.has("threads"));
        assertEquals("litematic", json.get("format").getAsString(), "for Litematica, whatever the server's default");
        assertEquals(19, json.size());
        assertTrue(json.get("voxel_size").isJsonNull(), "null is sent, meaning derive it");
        assertEquals(field("light_dir").defaultValue(), json.get("light_dir"));
        json.addProperty("max_size", 1);
        assertEquals(128, model.number("max_size"), "the JSON is a copy");
    }

    @Test
    void previewsAreCappedAndDropTheVoxelSize() {
        model.setNumber("voxel_size", 0.1);
        JsonObject preview = model.toPreviewJson(64);
        assertEquals(64, preview.get("max_size").getAsInt());
        assertEquals(JsonNull.INSTANCE, preview.get("voxel_size"));
        model.setNumber("max_size", 32);
        assertEquals(32, model.toPreviewJson(64).get("max_size").getAsInt());
        assertEquals(0.1, model.number("voxel_size"), "the form keeps its own value");
    }

    @Test
    void anglesRoundTrip() {
        for (int az = -170; az <= 180; az += 37) {
            for (int el = -80; el <= 80; el += 20) {
                model.setAngles("light_dir", az, el);
                assertEquals(az, model.azimuth("light_dir"), 1e-9, az + "/" + el);
                assertEquals(el, model.elevation("light_dir"), 1e-9, az + "/" + el);
                double[] v = model.direction("light_dir");
                assertEquals(1.0, Math.sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]), 1e-12, "stored as a unit vector");
            }
        }
    }

    @Test
    void anglesFollowTheWebUisConvention() {
        model.setAngles("light_dir", 90, 0);
        assertVector(1, 0, 0, model.direction("light_dir")); // +Z toward +X
        model.setAngles("light_dir", 0, 0);
        assertVector(0, 0, 1, model.direction("light_dir"));
        model.setAngles("light_dir", 0, 90);
        assertVector(0, 1, 0, model.direction("light_dir"));
        model.setAngles("light_dir", 180, -30);
        assertEquals(180, model.azimuth("light_dir"), 1e-9);
        assertEquals(-30, model.elevation("light_dir"), 1e-9);
    }

    @Test
    void theDefaultLightIsKeptExactly() {
        // The default (0.35, 0.85, 0.40) sits at 41° / 58° on whole-degree sliders.
        assertEquals(41, Math.round(model.azimuth("light_dir")));
        assertEquals(58, Math.round(model.elevation("light_dir")));
        model.setAngles("light_dir", 120, 10);
        model.setAngles("light_dir", 41, 58);
        assertEquals(field("light_dir").defaultValue(), model.get("light_dir"), "not a re-derived unit vector");
        model.setDirection("light_dir", 0.7, 1.7, 0.8); // the default, doubled
        assertEquals(field("light_dir").defaultValue(), model.get("light_dir"));
        assertThrows(IllegalArgumentException.class, () -> model.setDirection("light_dir", 0, 0, 0));
    }

    @Test
    void snapshotsHoldOnlyWhatThePlayerChanged() {
        model.setString("target", "1.20.4");
        model.setString("schematic_name", "castle");
        model.setNumber("max_size", 200);
        model.setBool("dither", true); // its default
        JsonObject saved = model.snapshot();
        assertEquals(1, saved.size(), saved.toString());
        assertEquals(200, saved.get("max_size").getAsInt());
        assertEquals(0, new SettingsModel(schema).snapshot().size());
    }

    @Test
    void restoreAppliesWhatStillFitsAndSkipsTheRest() {
        JsonObject saved = JsonParser.parseString("""
                {"max_size": 3000, "dither": false, "brightness": "bright", "voxel_size": null,
                 "default_block": "minecraft:netherrack", "target": "1.20.4", "no_longer_a_setting": 1,
                 "light_dir": [0, 2, 0], "ram_limit": [1]}
                """).getAsJsonObject();
        model.setNumber("brightness", 0.2);

        model.restore(saved);

        assertEquals(2048, model.number("max_size"), "clamped");
        assertFalse(model.bool("dither"));
        assertEquals(0.0, model.number("brightness"), "not a number: back to the default");
        assertTrue(model.isNull("voxel_size"));
        assertEquals("minecraft:netherrack", model.string("default_block"));
        assertEquals("1.21.8", model.string("target"), "the target is never restored");
        assertVector(0, 1, 0, model.direction("light_dir"));
        assertEquals(4.0, model.number("ram_limit"));
    }

    @Test
    void aSnapshotRestoresToTheSameSettings() {
        model.setNumber("max_size", 96);
        model.setBool("dither", false);
        model.setAngles("light_dir", -60, 30);
        model.setNumber("delight", 0.4);
        SettingsModel other = new SettingsModel(schema);
        other.restore(model.snapshot());
        JsonObject expected = model.toJson();
        JsonObject actual = other.toJson();
        JsonArray a = expected.remove("light_dir").getAsJsonArray();
        JsonArray b = actual.remove("light_dir").getAsJsonArray();
        assertEquals(expected, actual);
        for (int i = 0; i < 3; i++) {
            assertEquals(a.get(i).getAsDouble(), b.get(i).getAsDouble(), 1e-12);
        }
    }

    private static void assertVector(double x, double y, double z, double[] v) {
        assertEquals(x, v[0], 1e-12);
        assertEquals(y, v[1], 1e-12);
        assertEquals(z, v[2], 1e-12);
    }
}
