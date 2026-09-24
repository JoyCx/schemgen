package io.github.joycx.schemgen.common.schema;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.JsonParseException;
import com.google.gson.JsonPrimitive;
import io.github.joycx.schemgen.common.Fixtures;
import java.util.List;
import org.junit.jupiter.api.Test;

/** Against {@code fixtures/schema.json}: {@code GET /api/schema} of a real schemgen2 2.1.0. */
class SchemaTest {
    private final Schema schema = Fixtures.schema();

    @Test
    void groupsComeInTheWebUisOrder() {
        assertEquals(List.of("Size & shape", "Color", "Lighting", "Target version", "Output", "Advanced"),
                schema.groups().stream().map(Group::label).toList());
        assertTrue(schema.groups().get(2).help().startsWith("Separates lighting"));
        assertNull(schema.groups().get(0).help());
    }

    @Test
    void numericFieldsCarryTheirRanges() {
        Field maxSize = schema.field("max_size").orElseThrow();
        assertEquals(FieldType.INT, maxSize.type());
        assertEquals(Scope.CONVERSION, maxSize.scope());
        assertEquals(1.0, maxSize.min());
        assertEquals(2048.0, maxSize.max());
        assertEquals(1.0, maxSize.step());
        assertEquals("blocks", maxSize.unit());
        assertEquals(new JsonPrimitive(128), maxSize.defaultValue());
        assertArrayEquals(new double[] {8, 512}, maxSize.sliderRange(), "the slider spans less than min…max");

        Field delight = schema.field("delight").orElseThrow();
        assertArrayEquals(new double[] {0, 1}, delight.sliderRange(), "min…max when there is no slider");
    }

    @Test
    void nullableAndAdvancedFields() {
        Field voxel = schema.field("voxel_size").orElseThrow();
        assertTrue(voxel.nullable());
        assertTrue(voxel.advanced());
        assertTrue(voxel.defaultValue().isJsonNull());
        assertEquals("Auto", voxel.placeholder());
        assertNull(voxel.sliderRange(), "no maximum: a text box, not a slider");
        assertFalse(schema.field("max_size").orElseThrow().nullable());
    }

    @Test
    void conditionsChoicesAndDirections() {
        Condition when = schema.field("dither").orElseThrow().when();
        assertEquals("color_sampling", when.field());
        assertEquals(new JsonPrimitive(true), when.equals());

        Field block = schema.field("default_block").orElseThrow();
        assertEquals(FieldType.BLOCK, block.type());
        assertEquals(List.of("minecraft:white_concrete", "minecraft:netherrack"),
                block.choices().stream().map(c -> c.value().getAsString()).toList());

        Field light = schema.field("light_dir").orElseThrow();
        assertEquals(FieldType.DIRECTION, light.type());
        assertEquals(3, light.defaultValue().getAsJsonArray().size());

        Field target = schema.field("target").orElseThrow();
        assertEquals(FieldType.CHOICE, target.type());
        assertEquals("1.21.8", target.defaultValue().getAsString());
        assertEquals(schema.targets().size(), target.choices().size());
    }

    @Test
    void targetsFormatsPaletteAndLimits() {
        assertEquals("2.1.0", schema.version());
        assertEquals("26.3", schema.targets().get(0).id(), "newest first");
        assertTrue(schema.targets().contains(new Target("1.21.8", 4440, 7)));
        assertTrue(schema.targets().contains(new Target("1.16.5", 2586, 6)));
        assertEquals("1.21.8", schema.defaultTarget());
        assertEquals(List.of("litematic", "schem", "schem-v3", "nbt"), schema.formats().stream().map(Format::id).toList());
        assertEquals(new Format("litematic", "Litematica (.litematic)", "litematic"), schema.formats().get(0));
        assertEquals(181, schema.palette().entries());
        assertEquals(1L << 30, schema.limits().maxUploadBytes());
    }

    @Test
    void theModShowsNoFolderJobOrFormatField() {
        Field folder = schema.field("output_dir").orElseThrow();
        assertEquals(FieldType.FOLDER, folder.type());
        assertEquals(Scope.JOB, folder.scope());
        assertFalse(folder.isUsedByMod());
        assertFalse(schema.field("threads").orElseThrow().isUsedByMod());
        assertFalse(schema.field("format").orElseThrow().isUsedByMod(), "always .litematic");

        Group output = schema.groups().get(4);
        assertEquals(List.of("schematic_name"), schema.fieldsIn(output).stream().map(Field::key).toList());
        Group advanced = schema.groups().get(5);
        assertEquals(List.of("ram_limit"), schema.fieldsIn(advanced).stream().map(Field::key).toList());
        assertEquals(List.of("max_size", "voxel_size"),
                schema.fieldsIn(schema.groups().get(0)).stream().map(Field::key).toList());
    }

    @Test
    void anUnknownFieldTypeIsSkippedNotFatal() {
        Schema future = Schema.parse("""
                {"groups":[{"key":"g","label":"G"}],
                 "fields":[{"key":"tint","type":"color","group":"g","label":"Tint","help":"","default":"#fff","scope":"conversion"},
                           {"key":"n","type":"int","group":"g","label":"N","help":"","default":1,"scope":"conversion"}]}
                """);
        assertNull(future.field("tint").orElseThrow().type());
        assertEquals(List.of("n"), future.fieldsIn(future.groups().get(0)).stream().map(Field::key).toList());
        assertEquals(List.of(), future.targets());
    }

    @Test
    void somethingElseIsNotASchema() {
        assertThrows(JsonParseException.class, () -> Schema.parse("{\"status\":\"ok\"}"));
        assertThrows(JsonParseException.class, () -> Schema.parse("not json"));
    }
}
