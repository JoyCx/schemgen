package com.schemgen.mod.gui;

import com.schemgen.mod.ApiClient;
import com.schemgen.mod.ConversionTask;
import com.schemgen.mod.FilePicker;
import com.schemgen.mod.SchemGenConfig;
import com.schemgen.mod.SchemGenMod;
import com.schemgen.mod.SchematicsFolder;

import net.minecraft.client.gui.DrawContext;
import net.minecraft.client.gui.screen.Screen;
import net.minecraft.client.gui.widget.ButtonWidget;
import net.minecraft.client.gui.widget.CyclingButtonWidget;
import net.minecraft.client.gui.widget.SliderWidget;
import net.minecraft.client.gui.widget.TextFieldWidget;
import net.minecraft.text.Text;
import net.minecraft.util.Formatting;
import net.minecraft.util.Util;

import java.nio.file.Files;
import java.nio.file.Path;

/**
 * The converter GUI: pick a model, set the few settings that matter in game,
 * convert, and the finished schematic is written into this instance's
 * {@code schematics} folder.
 *
 * <p>All the work happens in {@link ConversionTask} on its own thread; this
 * class only reads its progress each frame. Nothing here blocks.
 */
public final class SchemGenScreen extends Screen {

    private static final int ROW_HEIGHT = 24;
    private static final int PANEL_WIDTH = 300;

    private static final int COLOR_TEXT = 0xFFE7E7E7;
    private static final int COLOR_MUTED = 0xFF9A9A9A;
    private static final int COLOR_ERROR = 0xFFE06C6C;
    private static final int COLOR_OK = 0xFF7BD88F;
    private static final int COLOR_BAR_BG = 0xFF2A2A2A;
    private static final int COLOR_BAR_FILL = 0xFF4E9AF1;

    private final SchemGenConfig config = SchemGenMod.config();

    private TextFieldWidget modelField;
    private TextFieldWidget nameField;
    private ButtonWidget convertButton;
    private ButtonWidget cancelButton;

    private ConversionTask task;
    /** Set when the user's input is wrong, as opposed to the conversion failing. */
    private String inputError;

    public SchemGenScreen() {
        super(Text.literal("SchemGen2 — model to schematic"));
    }

    @Override
    protected void init() {
        int left = this.width / 2 - PANEL_WIDTH / 2;
        int top = 46;
        int fieldWidth = PANEL_WIDTH - 76;

        String previousModel = modelField == null ? "" : modelField.getText();
        String previousName = nameField == null ? "" : nameField.getText();

        modelField = new TextFieldWidget(this.textRenderer, left, top, fieldWidth, 20,
                Text.literal("Path to a .glb or .gltf file"));
        modelField.setMaxLength(512);
        modelField.setText(previousModel);
        modelField.setChangedListener(value -> {
            inputError = null;
            if (nameField != null && nameField.getText().isBlank()) {
                nameField.setPlaceholder(Text.literal(defaultName(value)));
            }
        });
        this.addDrawableChild(modelField);

        this.addDrawableChild(ButtonWidget.builder(Text.literal("Browse..."), button -> browse())
                .dimensions(left + fieldWidth + 4, top, 72, 20)
                .build());

        nameField = new TextFieldWidget(this.textRenderer, left, top + ROW_HEIGHT, PANEL_WIDTH, 20,
                Text.literal("Schematic name"));
        nameField.setMaxLength(96);
        nameField.setText(previousName);
        nameField.setPlaceholder(Text.literal(defaultName(modelField.getText())));
        this.addDrawableChild(nameField);

        this.addDrawableChild(new IntSlider(left, top + ROW_HEIGHT * 2, PANEL_WIDTH, 20,
                "Longest side", 16, 512, config.maxSize, value -> {
            config.maxSize = value;
            config.save();
        }));

        this.addDrawableChild(CyclingButtonWidget.onOffBuilder(config.dither)
                .build(left, top + ROW_HEIGHT * 3, PANEL_WIDTH / 2 - 2, 20,
                        Text.literal("Dithering"), (button, value) -> {
                            config.dither = value;
                            config.save();
                        }));

        this.addDrawableChild(new PercentSlider(left + PANEL_WIDTH / 2 + 2, top + ROW_HEIGHT * 3,
                PANEL_WIDTH / 2 - 2, 20, "De-light", config.delight, value -> {
            config.delight = value;
            config.save();
        }));

        convertButton = this.addDrawableChild(ButtonWidget.builder(Text.literal("Convert"), button -> convert())
                .dimensions(left, top + ROW_HEIGHT * 4 + 30, PANEL_WIDTH / 2 - 2, 20)
                .build());

        cancelButton = this.addDrawableChild(ButtonWidget.builder(Text.literal("Cancel"), button -> cancel())
                .dimensions(left + PANEL_WIDTH / 2 + 2, top + ROW_HEIGHT * 4 + 30, PANEL_WIDTH / 2 - 2, 20)
                .build());

        this.addDrawableChild(ButtonWidget.builder(Text.literal("Open schematics folder"), button -> openFolder())
                .dimensions(left, top + ROW_HEIGHT * 5 + 32, PANEL_WIDTH / 2 - 2, 20)
                .build());

        this.addDrawableChild(ButtonWidget.builder(Text.literal("Close"), button -> this.close())
                .dimensions(left + PANEL_WIDTH / 2 + 2, top + ROW_HEIGHT * 5 + 32, PANEL_WIDTH / 2 - 2, 20)
                .build());

        refreshButtons();
    }

    @Override
    public void render(DrawContext context, int mouseX, int mouseY, float delta) {
        super.render(context, mouseX, mouseY, delta);

        int left = this.width / 2 - PANEL_WIDTH / 2;
        int titleX = this.width / 2 - this.textRenderer.getWidth(this.title) / 2;
        context.drawText(this.textRenderer, this.title, titleX, 18, COLOR_TEXT, false);

        context.drawText(this.textRenderer, Text.literal("Server: " + config.serverUrl),
                left, 32, COLOR_MUTED, false);

        int barY = 46 + ROW_HEIGHT * 4 + 6;
        context.fill(left, barY, left + PANEL_WIDTH, barY + 6, COLOR_BAR_BG);

        if (task != null) {
            int filled = (int) (PANEL_WIDTH * Math.clamp(task.percent(), 0.0f, 1.0f));
            context.fill(left, barY, left + filled, barY + 6, COLOR_BAR_FILL);
        }

        context.drawText(this.textRenderer, Text.literal(statusLine()),
                left, barY + 12, statusColor(), false);
    }

    /**
     * A running conversion is deliberately not tied to this screen: closing it
     * leaves the task running, and it still saves.
     */
    @Override
    public void tick() {
        refreshButtons();
    }

    // ---- actions -----------------------------------------------------------

    private void browse() {
        FilePicker.choose(config.lastDirectory, chosen -> SchemGenMod.onClientThread(() -> {
            if (chosen == null) {
                return;
            }
            modelField.setText(chosen.toString());
            inputError = null;
            if (chosen.getParent() != null) {
                config.lastDirectory = chosen.getParent().toString();
                config.save();
            }
            if (nameField.getText().isBlank()) {
                nameField.setPlaceholder(Text.literal(defaultName(chosen.toString())));
            }
        }));
    }

    private void convert() {
        if (task != null && !task.finished()) {
            return;
        }

        String raw = modelField.getText().trim().replaceAll("^\"|\"$", "");
        if (raw.isEmpty()) {
            inputError = "Pick a .glb or .gltf file first";
            return;
        }

        Path model;
        try {
            model = Path.of(raw);
        } catch (RuntimeException e) {
            inputError = "That is not a usable path";
            return;
        }
        String lower = raw.toLowerCase();
        if (!lower.endsWith(".glb") && !lower.endsWith(".gltf")) {
            inputError = "Only .glb and .gltf models can be converted";
            return;
        }
        if (!Files.isRegularFile(model)) {
            inputError = "No file at " + raw;
            return;
        }

        String name = nameField.getText().isBlank() ? defaultName(raw) : nameField.getText();
        inputError = null;
        task = new ConversionTask(config.serverUrl, model,
                new ApiClient.Settings(config.maxSize, config.dither, config.delight,
                        SchematicsFolder.sanitize(name)));
        task.start();
        refreshButtons();
    }

    private void cancel() {
        if (task != null && !task.finished()) {
            task.cancel();
            task = null;
            inputError = "Cancelled — the server may still be finishing that job";
        }
        refreshButtons();
    }

    private void openFolder() {
        Path folder = SchematicsFolder.path();
        try {
            Files.createDirectories(folder);
            Util.getOperatingSystem().open(folder.toUri());
        } catch (Exception e) {
            inputError = "Could not open " + folder + ": " + e.getMessage();
        }
    }

    // ---- presentation ------------------------------------------------------

    private void refreshButtons() {
        boolean running = task != null && !task.finished();
        if (convertButton != null) {
            convertButton.active = !running;
        }
        if (cancelButton != null) {
            cancelButton.active = running;
        }
    }

    private String statusLine() {
        if (inputError != null) {
            return inputError;
        }
        if (task == null) {
            return "Saves into " + SchematicsFolder.path();
        }
        return switch (task.stage()) {
            case DONE -> task.message();
            case FAILED -> task.message();
            default -> Math.round(task.percent() * 100) + "%  " + task.message();
        };
    }

    private int statusColor() {
        if (inputError != null) {
            return COLOR_ERROR;
        }
        if (task == null) {
            return COLOR_MUTED;
        }
        return switch (task.stage()) {
            case DONE -> COLOR_OK;
            case FAILED -> COLOR_ERROR;
            default -> COLOR_TEXT;
        };
    }

    /** The model's file name without its extension, as the web UI does. */
    private static String defaultName(String modelPath) {
        String trimmed = modelPath == null ? "" : modelPath.trim();
        if (trimmed.isEmpty()) {
            return "schemgen";
        }
        int slash = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
        String file = trimmed.substring(slash + 1);
        int dot = file.lastIndexOf('.');
        return SchematicsFolder.sanitize(dot > 0 ? file.substring(0, dot) : file);
    }

    /** Slider over an integer range, with the value shown in its label. */
    private static final class IntSlider extends SliderWidget {
        private final String label;
        private final int min;
        private final int max;
        private final java.util.function.IntConsumer onChange;

        IntSlider(int x, int y, int width, int height, String label, int min, int max, int initial,
                  java.util.function.IntConsumer onChange) {
            super(x, y, width, height, Text.literal(label),
                    (double) (Math.clamp(initial, min, max) - min) / (max - min));
            this.label = label;
            this.min = min;
            this.max = max;
            this.onChange = onChange;
            updateMessage();
        }

        private int intValue() {
            return min + (int) Math.round(this.value * (max - min));
        }

        @Override
        protected void updateMessage() {
            setMessage(Text.literal(label + ": " + intValue() + " blocks")
                    .formatted(Formatting.WHITE));
        }

        @Override
        protected void applyValue() {
            onChange.accept(intValue());
        }
    }

    /** Slider over 0..1, shown as a percentage. */
    private static final class PercentSlider extends SliderWidget {
        private final String label;
        private final java.util.function.Consumer<Float> onChange;

        PercentSlider(int x, int y, int width, int height, String label, float initial,
                      java.util.function.Consumer<Float> onChange) {
            super(x, y, width, height, Text.literal(label), Math.clamp(initial, 0.0f, 1.0f));
            this.label = label;
            this.onChange = onChange;
            updateMessage();
        }

        @Override
        protected void updateMessage() {
            setMessage(Text.literal(label + ": " + Math.round(this.value * 100) + "%")
                    .formatted(Formatting.WHITE));
        }

        @Override
        protected void applyValue() {
            onChange.accept((float) this.value);
        }
    }
}
