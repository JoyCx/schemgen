package io.github.joycx.schemgen.ui;

import io.github.joycx.schemgen.SchemGenSession;
import io.github.joycx.schemgen.common.schema.Choice;
import io.github.joycx.schemgen.common.schema.Condition;
import io.github.joycx.schemgen.common.schema.Field;
import io.github.joycx.schemgen.common.schema.FieldType;
import io.github.joycx.schemgen.common.schema.Group;
import io.github.joycx.schemgen.common.schema.Schema;
import io.github.joycx.schemgen.common.settings.SettingsModel;
import java.math.BigDecimal;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Objects;
import java.util.Set;
import java.util.function.Consumer;
import java.util.function.Supplier;
import java.util.stream.Collectors;
import net.minecraft.client.font.TextRenderer;
import net.minecraft.client.gui.DrawContext;
import net.minecraft.client.gui.tooltip.Tooltip;
import net.minecraft.client.gui.widget.ButtonWidget;
import net.minecraft.client.gui.widget.ClickableWidget;
import net.minecraft.client.gui.widget.TextFieldWidget;
import net.minecraft.text.Text;
import net.minecraft.util.Formatting;

/**
 * The settings column, rendered from the server's schema: its sections in
 * the schema's order — the web UI's — each field as the widget its type
 * calls for, and advanced fields behind a toggle. Taller than the screen, it
 * scrolls. It is built again with the screen whenever a field that another
 * field's visibility depends on changes.
 */
final class SettingsPanel {
    private static final int GAP = 4;
    private static final int WIDGET_HEIGHT = 20;

    /** Something at a height in the column: a widget, a line of text, or both side by side. */
    private record Item(ClickableWidget widget, Text text, boolean heading, Text help, int y, int height) {}

    private final int x;
    private final int top;
    private final int width;
    private final int height;
    private final List<Item> items = new ArrayList<>();
    private int contentHeight;
    private double scroll;

    SettingsPanel(int x, int top, int width, int height, double scroll) {
        this.x = x;
        this.top = top;
        this.width = width;
        this.height = height;
        this.scroll = scroll;
    }

    /**
     * Create the widgets for the session's settings, handing each to
     * {@code add} (the screen's {@code addDrawableChild}).
     *
     * @param choosingTarget show the target as a choice rather than as the game's version
     * @param chooseTarget switch to choosing the target
     */
    void build(SchemGenSession session, TextRenderer font, boolean choosingTarget, Runnable chooseTarget,
            Consumer<ClickableWidget> add) {
        SettingsModel model = session.settings();
        Schema schema = model.schema();
        // A change to one of these can show or hide other fields: the screen rebuilds.
        Set<String> conditions = schema.fields().stream()
                .map(Field::when).filter(Objects::nonNull).map(Condition::field)
                .collect(Collectors.toSet());
        int y = 0;
        for (Group group : schema.groups()) {
            List<Field> fields = schema.fieldsIn(group).stream()
                    .filter(f -> model.isVisible(f) && (session.showAdvanced() || !f.advanced()))
                    .toList();
            if (fields.isEmpty()) {
                continue;
            }
            Text help = group.help() == null ? null : Text.literal(group.help());
            y = text(Text.literal(group.label()).formatted(Formatting.BOLD), true, help, y + (y == 0 ? 0 : GAP));
            for (Field field : fields) {
                Runnable changed = conditions.contains(field.key()) ? session::changed : () -> {};
                y = field(field, model, session, font, choosingTarget, chooseTarget, changed, add, y);
            }
        }
        boolean advanced = session.showAdvanced();
        ButtonWidget toggle = ButtonWidget.builder(
                        Text.translatable("schemgen.settings.advanced",
                                Text.translatable(advanced ? "schemgen.settings.shown" : "schemgen.settings.hidden")),
                        button -> session.setShowAdvanced(!advanced))
                .dimensions(x, 0, width, WIDGET_HEIGHT)
                .build();
        y = widget(toggle, null, add, y + GAP);
        contentHeight = y;
        scrollBy(0);
    }

    private int field(Field field, SettingsModel model, SchemGenSession session, TextRenderer font,
            boolean choosingTarget, Runnable chooseTarget, Runnable changed, Consumer<ClickableWidget> add, int y) {
        String key = field.key();
        Tooltip help = field.help() == null || field.help().isBlank() ? null : Tooltip.of(Text.literal(field.help()));
        if (key.equals("target") && field.type() == FieldType.CHOICE) {
            return target(field, model, session, choosingTarget, chooseTarget, help, add, y);
        }
        switch (field.type()) {
            case INT, FLOAT -> {
                double[] range = field.sliderRange();
                if (range != null) {
                    double step = field.step() != null && field.step() > 0 ? field.step() : field.type() == FieldType.INT ? 1 : 0;
                    SettingSlider slider = new SettingSlider(x, 0, width, range[0], range[1], step, new SettingSlider.Binding() {
                        @Override
                        public double get() {
                            return model.number(key);
                        }

                        @Override
                        public void set(double value) {
                            model.setNumber(key, value);
                            changed.run();
                        }

                        @Override
                        public Text label(double value) {
                            String unit = field.unit() == null ? "" : " " + field.unit();
                            return Text.literal(field.label() + ": " + format(field, value) + unit);
                        }
                    });
                    return widget(slider, help, add, y);
                }
                // An open range, such as the voxel size: typed, and empty means null.
                y = text(Text.literal(field.label()), false, null, y);
                TextFieldWidget box = textBox(font, field);
                box.setText(model.isNull(key) ? "" : plain(model.number(key)));
                box.setChangedListener(text -> {
                    String value = text.strip();
                    try {
                        if (value.isEmpty()) {
                            if (field.nullable()) {
                                model.setNull(key);
                            }
                        } else {
                            model.setNumber(key, Double.parseDouble(value));
                        }
                    } catch (IllegalArgumentException notANumber) {
                        // Keep the last valid value while the player types.
                    }
                });
                return widget(box, help, add, y);
            }
            case BOOL -> {
                Supplier<Text> message = () -> Text.translatable("schemgen.settings.toggle", field.label(),
                        Text.translatable(model.bool(key) ? "options.on" : "options.off"));
                ButtonWidget button = ButtonWidget.builder(message.get(), b -> {
                    model.setBool(key, !model.bool(key));
                    b.setMessage(message.get());
                    changed.run();
                }).dimensions(x, 0, width, WIDGET_HEIGHT).build();
                return widget(button, help, add, y);
            }
            case CHOICE, BLOCK -> {
                List<Choice> choices = field.choices();
                if (choices.isEmpty()) {
                    return y; // nothing to cycle through
                }
                Supplier<Text> message = () -> Text.literal(field.label() + ": " + choiceLabel(field, model));
                ButtonWidget button = ButtonWidget.builder(message.get(), b -> {
                    int current = indexOf(choices, model.string(key));
                    model.setString(key, choices.get((current + 1) % choices.size()).value().getAsString());
                    b.setMessage(message.get());
                    changed.run();
                }).dimensions(x, 0, width, WIDGET_HEIGHT).build();
                return widget(button, help, add, y);
            }
            case TEXT -> {
                y = text(Text.literal(field.label()), false, null, y);
                TextFieldWidget box = textBox(font, field);
                box.setText(model.string(key));
                box.setChangedListener(text -> model.setString(key, text));
                return widget(box, help, add, y);
            }
            case DIRECTION -> {
                y = widget(new SettingSlider(x, 0, width, -180, 180, 1, new SettingSlider.Binding() {
                    @Override
                    public double get() {
                        return model.azimuth(key);
                    }

                    @Override
                    public void set(double value) {
                        model.setAngles(key, value, model.elevation(key));
                    }

                    @Override
                    public Text label(double value) {
                        return Text.translatable("schemgen.direction.azimuth", field.label(), Math.round(value));
                    }
                }), help, add, y);
                return widget(new SettingSlider(x, 0, width, -90, 90, 1, new SettingSlider.Binding() {
                    @Override
                    public double get() {
                        return model.elevation(key);
                    }

                    @Override
                    public void set(double value) {
                        model.setAngles(key, model.azimuth(key), value);
                    }

                    @Override
                    public Text label(double value) {
                        return Text.translatable("schemgen.direction.elevation", field.label(), Math.round(value));
                    }
                }), help, add, y);
            }
            default -> {
                return y; // folders are the server's business; the mod saves files itself
            }
        }
    }

    /**
     * The target follows the game, so it shows as the game's version with a
     * way to change it; once changing, it cycles through the server's
     * targets, "this game" first.
     */
    private int target(Field field, SettingsModel model, SchemGenSession session, boolean choosing,
            Runnable chooseTarget, Tooltip help, Consumer<ClickableWidget> add, int y) {
        boolean followsGame = session.config().targetOverride.isBlank();
        if (followsGame && !choosing) {
            int buttonWidth = 60;
            ButtonWidget change = ButtonWidget.builder(Text.translatable("schemgen.target.change"), b -> chooseTarget.run())
                    .dimensions(x + width - buttonWidth, 0, buttonWidth, WIDGET_HEIGHT)
                    .build();
            if (help != null) {
                change.setTooltip(help);
            }
            add.accept(change);
            items.add(new Item(change, Text.translatable("schemgen.target.this_game", field.label(), model.string("target")),
                    false, null, y, WIDGET_HEIGHT));
            return y + WIDGET_HEIGHT + GAP;
        }
        List<String> values = new ArrayList<>();
        values.add(""); // this game
        field.choices().forEach(choice -> values.add(choice.value().getAsString()));
        Text message = followsGame
                ? Text.translatable("schemgen.target.cycle_game", field.label(), model.string("target"))
                : Text.literal(field.label() + ": " + model.string("target"));
        ButtonWidget button = ButtonWidget.builder(message, b -> {
            int current = values.indexOf(session.config().targetOverride);
            session.setTargetOverride(values.get((current + 1) % values.size()));
        }).dimensions(x, 0, width, WIDGET_HEIGHT).build();
        return widget(button, help, add, y);
    }

    private TextFieldWidget textBox(TextRenderer font, Field field) {
        TextFieldWidget box = new TextFieldWidget(font, x, 0, width, WIDGET_HEIGHT, Text.literal(field.label()));
        box.setMaxLength(200);
        if (field.placeholder() != null) {
            box.setPlaceholder(Text.literal(field.placeholder()).formatted(Formatting.DARK_GRAY));
        }
        return box;
    }

    private int widget(ClickableWidget widget, Tooltip help, Consumer<ClickableWidget> add, int y) {
        if (help != null) {
            widget.setTooltip(help);
        }
        add.accept(widget);
        items.add(new Item(widget, null, false, null, y, WIDGET_HEIGHT));
        return y + WIDGET_HEIGHT + GAP;
    }

    private int text(Text text, boolean heading, Text help, int y) {
        int lineHeight = heading ? 13 : 11;
        items.add(new Item(null, text, heading, help, y, lineHeight));
        return y + lineHeight;
    }

    /** Scroll by {@code amount} pixels (positive: down) and put every widget where it now belongs. */
    void scrollBy(double amount) {
        scroll = Math.max(0, Math.min(Math.max(0, contentHeight - height), scroll + amount));
        for (Item item : items) {
            if (item.widget() != null) {
                int y = screenY(item);
                item.widget().setY(y);
                item.widget().visible = y >= top && y + item.height() <= top + height;
            }
        }
    }

    double scroll() {
        return scroll;
    }

    boolean contains(double mouseX, double mouseY) {
        return mouseX >= x && mouseX < x + width + 6 && mouseY >= top && mouseY < top + height;
    }

    /** The headings and labels, the scrollbar, and a heading's help while hovered. */
    void render(DrawContext context, TextRenderer font, int mouseX, int mouseY) {
        Text hovered = null;
        for (Item item : items) {
            int y = screenY(item);
            if (item.text() == null || y < top || y + item.height() > top + height) {
                continue;
            }
            int textY = item.widget() != null ? y + (WIDGET_HEIGHT - font.fontHeight) / 2 : y + 1;
            int color = item.heading() ? 0xFFFFFFFF : 0xFFC0C0C0;
            Text text = item.help() == null ? item.text() : item.text().copy().append(Text.literal(" (?)").formatted(Formatting.GRAY));
            context.drawTextWithShadow(font, text, x, textY, color);
            if (item.help() != null && mouseX >= x && mouseX < x + font.getWidth(text) && mouseY >= y && mouseY < y + item.height()) {
                hovered = item.help();
            }
        }
        if (contentHeight > height) {
            int barHeight = Math.max(16, height * height / contentHeight);
            int barY = top + (int) ((height - barHeight) * scroll / (contentHeight - height));
            context.fill(x + width + 2, top, x + width + 4, top + height, 0x40FFFFFF);
            context.fill(x + width + 2, barY, x + width + 4, barY + barHeight, 0xC0FFFFFF);
        }
        if (hovered != null) {
            context.drawOrderedTooltip(font, font.wrapLines(hovered, 200), mouseX, mouseY);
        }
    }

    private int screenY(Item item) {
        return top + item.y() - (int) scroll;
    }

    private static int indexOf(List<Choice> choices, String value) {
        for (int i = 0; i < choices.size(); i++) {
            if (choices.get(i).value().getAsString().equals(value)) {
                return i;
            }
        }
        return -1;
    }

    private static String choiceLabel(Field field, SettingsModel model) {
        String value = model.string(field.key());
        return field.choices().stream()
                .filter(choice -> choice.value().getAsString().equals(value))
                .map(Choice::label)
                .findFirst()
                .orElse(value);
    }

    /** A slider's value, to the precision of its step. */
    static String format(Field field, double value) {
        if (Double.isNaN(value)) {
            return "–";
        }
        if (field.type() == FieldType.INT) {
            return Long.toString(Math.round(value));
        }
        Double step = field.step();
        int decimals = step == null || step >= 1 ? 0 : (int) Math.ceil(-Math.log10(step) - 1e-9);
        return String.format(Locale.ROOT, "%." + decimals + "f", value);
    }

    /** A typed number as a person would write it: 0.25, not 0.25000000. */
    private static String plain(double value) {
        return BigDecimal.valueOf(value).stripTrailingZeros().toPlainString();
    }
}
