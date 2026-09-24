package io.github.joycx.schemgen.ui;

import io.github.joycx.schemgen.SchemGenSession;
import io.github.joycx.schemgen.common.config.ModConfig;
import io.github.joycx.schemgen.common.config.ServerMode;
import java.nio.file.InvalidPathException;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.client.gui.DrawContext;
import net.minecraft.client.gui.screen.Screen;
import net.minecraft.client.gui.tooltip.Tooltip;
import net.minecraft.client.gui.widget.ButtonWidget;
import net.minecraft.client.gui.widget.TextFieldWidget;
import net.minecraft.text.Text;
import net.minecraft.util.Formatting;

/**
 * {@code schemgen.json} as a form: which server to use, and where models and
 * schematics live. Saving a changed server lets the old one go — a sidecar
 * is stopped — and connects again; everything else applies at once.
 */
public final class ServerSettingsScreen extends Screen {
    private static final int PAD = 8;
    private static final int ROW = 20;
    private static final int LABEL = 11;
    private static final int GAP = 4;
    private static final int WHITE = 0xFFFFFFFF;
    private static final int GRAY = 0xFFA0A0A0;
    private static final int RED = 0xFFFF6060;

    private record Label(Text text, int x, int y, int color) {}

    private final Screen parent;
    private final SchemGenSession session;
    /** The form's working copy, kept while the mode switch rebuilds the form. */
    private final ModConfig draft;
    private String portText;
    private Text error = Text.empty();

    private final List<Label> labels = new ArrayList<>();
    private TextFieldWidget host;
    private TextFieldWidget port;
    private TextFieldWidget token;
    private TextFieldWidget binary;
    private TextFieldWidget models;
    private TextFieldWidget output;
    private int errorY;

    ServerSettingsScreen(Screen parent, SchemGenSession session) {
        super(Text.translatable("schemgen.server.title"));
        this.parent = parent;
        this.session = session;
        this.draft = session.config().sanitized();
        this.portText = Integer.toString(draft.port);
    }

    @Override
    protected void init() {
        labels.clear();
        host = port = token = binary = null;
        int column = Math.min(200, (width - 3 * PAD) / 2);
        int leftX = width / 2 - PAD / 2 - column;
        int rightX = width / 2 + PAD / 2;
        int y = 30;

        // The server.
        label("schemgen.server.section.server", leftX, y, WHITE);
        int left = y + LABEL + 2;
        addDrawableChild(ButtonWidget.builder(modeLabel(), b -> {
                    readFields();
                    draft.serverMode = draft.serverMode == ServerMode.SIDECAR ? ServerMode.EXTERNAL : ServerMode.SIDECAR;
                    clearAndInit();
                })
                .dimensions(leftX, left, column, ROW)
                .tooltip(Tooltip.of(Text.translatable("schemgen.server.mode.tooltip")))
                .build());
        left += ROW + GAP;
        if (draft.serverMode == ServerMode.EXTERNAL) {
            label("schemgen.server.address", leftX, left, GRAY);
            left += LABEL;
            int portWidth = 50;
            host = field(leftX, left, column - portWidth - GAP, draft.host, "127.0.0.1");
            port = field(leftX + column - portWidth, left, portWidth, portText, Integer.toString(ModConfig.DEFAULT_PORT));
            port.setTextPredicate(text -> text.chars().allMatch(Character::isDigit));
            left += ROW + GAP;
            label("schemgen.server.token", leftX, left, GRAY);
            left += LABEL;
            token = field(leftX, left, column, draft.token, Text.translatable("schemgen.server.token.none").getString());
        } else {
            label("schemgen.server.binary", leftX, left, GRAY);
            left += LABEL;
            binary = field(leftX, left, column, draft.binaryPath, Text.translatable("schemgen.server.binary.default").getString());
        }
        left += ROW + GAP;

        // Files and previews.
        label("schemgen.server.section.files", rightX, y, WHITE);
        int right = y + LABEL + 2;
        label("schemgen.server.models", rightX, right, GRAY);
        right += LABEL;
        models = field(rightX, right, column, draft.modelsFolder, "schemgen/models");
        right += ROW + GAP;
        label("schemgen.server.output", rightX, right, GRAY);
        right += LABEL;
        output = field(rightX, right, column, draft.outputFolder, "schematics");
        right += ROW + GAP;
        addDrawableChild(ButtonWidget.builder(autoLoadLabel(), b -> {
                    draft.autoLoadIntoLitematica = !draft.autoLoadIntoLitematica;
                    b.setMessage(autoLoadLabel());
                })
                .dimensions(rightX, right, column, ROW)
                .tooltip(Tooltip.of(Text.translatable("schemgen.server.autoload.tooltip")))
                .build());
        right += ROW + GAP;
        SettingSlider previewSize = new SettingSlider(rightX, right, column, 8, ModConfig.PREVIEW_MAX_SIZE_LIMIT, 8,
                new SettingSlider.Binding() {
                    @Override
                    public double get() {
                        return draft.previewMaxSize;
                    }

                    @Override
                    public void set(double value) {
                        draft.previewMaxSize = (int) Math.round(value);
                    }

                    @Override
                    public Text label(double value) {
                        return Text.translatable("schemgen.server.preview_size", Math.round(value));
                    }
                });
        previewSize.setTooltip(Tooltip.of(Text.translatable("schemgen.server.preview_size.tooltip")));
        addDrawableChild(previewSize);
        right += ROW + GAP;

        errorY = Math.max(left, right) + 2;
        int buttonsY = Math.max(errorY + LABEL + GAP, Math.min(height - PAD - ROW, errorY + 40));
        addDrawableChild(ButtonWidget.builder(Text.translatable("schemgen.server.save"), b -> save())
                .dimensions(width / 2 - PAD / 2 - 100, buttonsY, 100, ROW)
                .build());
        addDrawableChild(ButtonWidget.builder(Text.translatable("gui.cancel"), b -> close())
                .dimensions(width / 2 + PAD / 2, buttonsY, 100, ROW)
                .build());
    }

    private TextFieldWidget field(int x, int y, int width, String value, String placeholder) {
        TextFieldWidget field = new TextFieldWidget(textRenderer, x, y, width, ROW, Text.empty());
        field.setMaxLength(1024);
        field.setText(value);
        field.setPlaceholder(Text.literal(placeholder).formatted(Formatting.DARK_GRAY));
        return addDrawableChild(field);
    }

    private void label(String key, int x, int y, int color) {
        labels.add(new Label(Text.translatable(key), x, y, color));
    }

    private Text modeLabel() {
        return Text.translatable("schemgen.server.mode", Text.translatable(draft.serverMode == ServerMode.EXTERNAL
                ? "schemgen.server.mode.external" : "schemgen.server.mode.sidecar"));
    }

    private Text autoLoadLabel() {
        return Text.translatable("schemgen.server.autoload",
                Text.translatable(draft.autoLoadIntoLitematica ? "options.on" : "options.off"));
    }

    /** Copy what was typed into the draft; the fields of the other mode are not there to read. */
    private void readFields() {
        if (host != null) {
            draft.host = host.getText().strip();
            portText = port.getText().strip();
            draft.token = token.getText().strip();
        }
        if (binary != null) {
            draft.binaryPath = binary.getText().strip();
        }
        draft.modelsFolder = models.getText().strip();
        draft.outputFolder = output.getText().strip();
    }

    private void save() {
        readFields();
        String problem = problem();
        if (problem != null) {
            error = Text.literal(problem);
            return;
        }
        ModConfig current = session.config();
        // Settings the main screen saved since this one opened are not the form's to change.
        draft.lastSettings = current.lastSettings;
        draft.targetOverride = current.targetOverride;
        boolean serverChanged = draft.serverMode != current.serverMode
                || !draft.host.equals(current.host)
                || draft.port != current.port
                || !draft.token.equals(current.token)
                || !draft.binaryPath.equals(current.binaryPath);
        session.updateConfig(draft, serverChanged);
        close();
    }

    /** Why the form cannot be saved as it is, or {@code null}. */
    private String problem() {
        if (draft.serverMode == ServerMode.EXTERNAL) {
            int number;
            try {
                number = Integer.parseInt(portText);
            } catch (NumberFormatException e) {
                number = 0;
            }
            if (number < 1 || number > 65535) {
                return Text.translatable("schemgen.server.error.port").getString();
            }
            draft.port = number;
            if (!ModConfig.isValidHost(draft.host)) {
                return Text.translatable("schemgen.server.error.host", draft.host).getString();
            }
        }
        for (String path : List.of(draft.binaryPath, draft.modelsFolder, draft.outputFolder)) {
            try {
                Path.of(path);
            } catch (InvalidPathException e) {
                return Text.translatable("schemgen.server.error.path", path).getString();
            }
        }
        return null;
    }

    @Override
    public void render(DrawContext context, int mouseX, int mouseY, float delta) {
        super.render(context, mouseX, mouseY, delta);
        context.drawCenteredTextWithShadow(textRenderer, title, width / 2, 12, WHITE);
        for (Label label : labels) {
            context.drawTextWithShadow(textRenderer, label.text(), label.x(), label.y(), label.color());
        }
        if (!error.getString().isEmpty()) {
            context.drawCenteredTextWithShadow(textRenderer, error, width / 2, errorY, RED);
        }
    }

    @Override
    public void close() {
        client.setScreen(parent);
    }
}
