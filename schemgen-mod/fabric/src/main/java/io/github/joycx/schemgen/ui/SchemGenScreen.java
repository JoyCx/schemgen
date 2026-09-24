package io.github.joycx.schemgen.ui;

import io.github.joycx.schemgen.Converter;
import io.github.joycx.schemgen.SchemGenSession;
import io.github.joycx.schemgen.common.config.ServerMode;
import io.github.joycx.schemgen.compat.ScreenCompat;
import io.github.joycx.schemgen.litematica.LitematicaSupport;
import io.github.joycx.schemgen.preview.GhostPreview;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.client.gui.DrawContext;
import net.minecraft.client.gui.screen.Screen;
import net.minecraft.client.gui.tooltip.Tooltip;
import net.minecraft.client.gui.widget.ButtonWidget;
import net.minecraft.text.OrderedText;
import net.minecraft.text.Text;
import net.minecraft.util.Formatting;
import net.minecraft.util.Identifier;
import net.minecraft.util.Util;

/**
 * The mod's screen, laid out like the web UI: the models on the left with
 * the last result's thumbnail, the settings from the server's schema on the
 * right, and the progress, status and actions along the bottom.
 *
 * <p>Everything it shows lives in the {@link SchemGenSession}, so closing the
 * screen loses nothing and a conversion keeps running without it. Widgets
 * are rebuilt whenever the session's revision moves on.
 */
public final class SchemGenScreen extends Screen {
    private static final int PAD = 8;
    private static final int ROW = 20;
    private static final int GAP = 4;
    private static final int MODEL_ROW = 18;
    private static final int WHITE = 0xFFFFFFFF;
    private static final int GRAY = 0xFFA0A0A0;
    private static final int RED = 0xFFFF6060;

    private final SchemGenSession session;
    private int builtRevision;
    private boolean connectRequested;

    // Kept across rebuilds, so a change in the form does not jump the view.
    private double settingsScroll;
    private int firstModel;
    private boolean choosingTarget;

    // Geometry of the last layout.
    private int top;
    private int contentBottom;
    private int leftWidth;
    private int rightX;
    private int listTop;
    private int thumbnailSize;
    private int barY;
    private int statusY;

    private SettingsPanel settings;
    private final List<ButtonWidget> modelButtons = new ArrayList<>();
    private List<Path> entries = List.of();
    private ButtonWidget previewButton;
    private ButtonWidget rotateButton;
    private ButtonWidget clearButton;
    private ButtonWidget convertButton;
    private ButtonWidget loadButton;
    private ButtonWidget cancelButton;

    /** The newer of the conversion's and the preview's messages is the status line. */
    private Text seenConversionMessage;
    private Text seenPreviewMessage;
    private Text status = Text.empty();

    public SchemGenScreen(SchemGenSession session) {
        super(Text.translatable("schemgen.screen.title"));
        this.session = session;
    }

    @Override
    protected void init() {
        if (!connectRequested) {
            connectRequested = true; // once per opening: a failure waits for Retry instead of looping
            session.connect();
        }
        builtRevision = session.revision();

        top = PAD + ROW + 6;
        int buttonsY = height - PAD - ROW;
        statusY = buttonsY - GAP - textRenderer.fontHeight;
        barY = statusY - 3 - 3;
        contentBottom = barY - GAP;
        leftWidth = Math.max(110, Math.min(190, (int) (width * 0.3)));
        rightX = PAD + leftWidth + 12;

        addDrawableChild(ButtonWidget.builder(Text.translatable("schemgen.screen.server"),
                        b -> client.setScreen(new ServerSettingsScreen(this, session)))
                .dimensions(width - PAD - 80, PAD, 80, ROW)
                .tooltip(Tooltip.of(Text.translatable("schemgen.screen.server.tooltip")))
                .build());
        initModels();
        initSettings();
        initActions(buttonsY);
        updateActions();
    }

    // ── Left column: models and thumbnail ───────────────────────────────────

    private void initModels() {
        int y = top + textRenderer.fontHeight + 3;
        int third = (leftWidth - 2 * GAP) / 3;
        Path folder = session.modelsFolder();
        addDrawableChild(ButtonWidget.builder(Text.translatable("schemgen.models.refresh"), b -> session.refreshModels())
                .dimensions(PAD, y, third, ROW)
                .tooltip(Tooltip.of(Text.translatable("schemgen.models.refresh.tooltip")))
                .build());
        addDrawableChild(ButtonWidget.builder(Text.translatable("schemgen.models.folder"),
                        b -> Util.getOperatingSystem().open(folder))
                .dimensions(PAD + third + GAP, y, third, ROW)
                .tooltip(Tooltip.of(Text.translatable("schemgen.models.folder.tooltip", folder.toString())))
                .build());
        addDrawableChild(ButtonWidget.builder(Text.translatable("schemgen.models.browse"),
                        b -> FilePicker.browse(folder, session::select))
                .dimensions(PAD + 2 * (third + GAP), y, leftWidth - 2 * (third + GAP), ROW)
                .tooltip(Tooltip.of(Text.translatable("schemgen.models.browse.tooltip")))
                .build());
        listTop = y + ROW + GAP;

        boolean hasThumbnail = session.converter().thumbnail().id() != null;
        thumbnailSize = hasThumbnail ? Math.max(0, Math.min(leftWidth, (contentBottom - listTop) / 2)) : 0;
        int listBottom = contentBottom - (hasThumbnail ? thumbnailSize + GAP : 0);

        // A model chosen with Browse… from elsewhere is listed first.
        List<Path> models = new ArrayList<>(session.models());
        Path selected = session.selectedModel();
        if (selected != null && !models.contains(selected)) {
            models.add(0, selected);
        }
        entries = models;
        modelButtons.clear();
        int rows = Math.max(0, (listBottom - listTop) / MODEL_ROW);
        for (int row = 0; row < rows; row++) {
            int index = row;
            ButtonWidget button = ButtonWidget.builder(Text.empty(), b -> {
                        int i = firstModel + index;
                        if (i < entries.size()) {
                            session.select(entries.get(i));
                        }
                    })
                    .dimensions(PAD, listTop + row * MODEL_ROW, leftWidth, MODEL_ROW - 2)
                    .build();
            modelButtons.add(addDrawableChild(button));
        }
        scrollModels(0);
    }

    /** Scroll the model list by {@code rows} and relabel its buttons. */
    private void scrollModels(int rows) {
        firstModel = Math.max(0, Math.min(Math.max(0, entries.size() - modelButtons.size()), firstModel + rows));
        Path selected = session.selectedModel();
        for (int row = 0; row < modelButtons.size(); row++) {
            ButtonWidget button = modelButtons.get(row);
            int i = firstModel + row;
            button.visible = i < entries.size();
            if (!button.visible) {
                continue;
            }
            Path model = entries.get(i);
            String name = textRenderer.trimToWidth(model.getFileName().toString(), leftWidth - 10);
            button.setMessage(model.equals(selected) ? Text.literal(name).formatted(Formatting.YELLOW) : Text.literal(name));
            button.setTooltip(Tooltip.of(Text.literal(model.toString())));
        }
    }

    // ── Right column: the settings ──────────────────────────────────────────

    private void initSettings() {
        settings = null;
        if (session.settings() == null) {
            if (session.connection() == SchemGenSession.Connection.FAILED) {
                int y = top + 14 + 11 * errorLines().size();
                addDrawableChild(ButtonWidget.builder(Text.translatable("schemgen.screen.retry"), b -> session.connect())
                        .dimensions(rightX, y, 80, ROW)
                        .build());
            }
            return;
        }
        settings = new SettingsPanel(rightX, top, width - PAD - 6 - rightX, contentBottom - top, settingsScroll);
        settings.build(session, textRenderer, choosingTarget, () -> {
            choosingTarget = true;
            session.changed();
        }, this::addDrawableChild);
        settingsScroll = settings.scroll();
    }

    private List<OrderedText> errorLines() {
        return textRenderer.wrapLines(Text.literal(session.connectionError()), width - PAD - rightX);
    }

    // ── Bottom bar: the actions ─────────────────────────────────────────────

    private void initActions(int y) {
        GhostPreview preview = session.preview();
        Converter converter = session.converter();
        // Wider buttons for longer labels; the widths share the row.
        double[] weights = {1, 0.8, 0.8, 1, 1.5, 0.9};
        int[] x = new int[weights.length + 1];
        double total = 0;
        for (double w : weights) {
            total += w;
        }
        int available = width - 2 * PAD - GAP * (weights.length - 1);
        double at = 0;
        for (int i = 0; i < weights.length; i++) {
            x[i] = PAD + (int) Math.round(at * available / total) + i * GAP;
            at += weights[i];
        }
        x[weights.length] = width - PAD + GAP;

        previewButton = action(x, 0, y, "schemgen.action.preview", preview::request);
        rotateButton = action(x, 1, y, "schemgen.action.rotate", preview::rotate);
        clearButton = action(x, 2, y, "schemgen.action.clear", preview::clear);
        convertButton = action(x, 3, y, "schemgen.action.convert", converter::start);
        loadButton = action(x, 4, y, "schemgen.action.load", converter::loadLast);
        cancelButton = action(x, 5, y, "schemgen.action.cancel", converter::cancel);
        if (!LitematicaSupport.isLoaded()) {
            loadButton.setTooltip(Tooltip.of(Text.translatable("schemgen.litematica.missing")));
        }
    }

    private ButtonWidget action(int[] x, int i, int y, String key, Runnable onPress) {
        ButtonWidget button = ButtonWidget.builder(Text.translatable(key), b -> onPress.run())
                .dimensions(x[i], y, x[i + 1] - GAP - x[i], ROW)
                .tooltip(Tooltip.of(Text.translatable(key + ".tooltip")))
                .build();
        return addDrawableChild(button);
    }

    private void updateActions() {
        boolean ready = session.settings() != null && session.selectedModel() != null;
        GhostPreview preview = session.preview();
        Converter converter = session.converter();
        previewButton.active = ready && !preview.isBusy();
        rotateButton.active = preview.isShown();
        clearButton.active = preview.isShown();
        convertButton.active = ready && !converter.isRunning();
        loadButton.active = LitematicaSupport.isLoaded() && converter.lastSchematic() != null && !converter.isRunning();
        cancelButton.active = converter.canCancel();
    }

    // ── Frame ───────────────────────────────────────────────────────────────

    @Override
    public void tick() {
        if (session.revision() != builtRevision) {
            clearAndInit();
            return;
        }
        updateActions();
    }

    @Override
    public boolean mouseScrolled(double mouseX, double mouseY, double horizontalAmount, double verticalAmount) {
        if (settings != null && settings.contains(mouseX, mouseY)) {
            settings.scrollBy(-verticalAmount * 24);
            settingsScroll = settings.scroll();
            return true;
        }
        if (mouseX >= PAD && mouseX < PAD + leftWidth && mouseY >= listTop
                && mouseY < listTop + modelButtons.size() * MODEL_ROW) {
            scrollModels(verticalAmount > 0 ? -1 : 1);
            return true;
        }
        return super.mouseScrolled(mouseX, mouseY, horizontalAmount, verticalAmount);
    }

    @Override
    public void render(DrawContext context, int mouseX, int mouseY, float delta) {
        super.render(context, mouseX, mouseY, delta);
        context.drawTextWithShadow(textRenderer, title, PAD, PAD + 6, WHITE);
        context.drawTextWithShadow(textRenderer, serverStatus(), PAD + textRenderer.getWidth(title) + 10, PAD + 6, GRAY);

        renderModels(context);
        if (settings != null) {
            settings.render(context, textRenderer, mouseX, mouseY);
        } else {
            renderConnection(context);
        }
        renderStatus(context, mouseX, mouseY);
    }

    private void renderModels(DrawContext context) {
        context.drawTextWithShadow(textRenderer, Text.translatable("schemgen.models.title"), PAD, top, WHITE);
        if (entries.isEmpty()) {
            Text hint = Text.translatable("schemgen.models.empty", session.modelsFolder().toString());
            int y = listTop + 2;
            for (OrderedText line : textRenderer.wrapLines(hint, leftWidth)) {
                context.drawTextWithShadow(textRenderer, line, PAD, y, GRAY);
                y += textRenderer.fontHeight + 2;
            }
        }
        Identifier thumbnail = session.converter().thumbnail().id();
        if (thumbnail != null && thumbnailSize > 0) {
            int x = PAD + (leftWidth - thumbnailSize) / 2;
            int y = contentBottom - thumbnailSize;
            context.fill(x - 1, y - 1, x + thumbnailSize + 1, y + thumbnailSize + 1, 0x60000000);
            ScreenCompat.drawTexture(context, thumbnail, x, y, thumbnailSize, thumbnailSize);
        }
    }

    /** The right column while there is no form: starting, or why the server is not there. */
    private void renderConnection(DrawContext context) {
        if (session.connection() == SchemGenSession.Connection.FAILED) {
            context.drawTextWithShadow(textRenderer, Text.translatable("schemgen.screen.failed"), rightX, top, RED);
            int y = top + 14;
            for (OrderedText line : errorLines()) {
                context.drawTextWithShadow(textRenderer, line, rightX, y, WHITE);
                y += 11;
            }
            return;
        }
        Text waiting = session.config().serverMode == ServerMode.EXTERNAL
                ? Text.translatable("schemgen.screen.connecting", session.config().externalUri().toString())
                : Text.translatable("schemgen.screen.starting");
        context.drawTextWithShadow(textRenderer, waiting, rightX, top, GRAY);
    }

    private Text serverStatus() {
        return switch (session.connection()) {
            case READY -> session.config().serverMode == ServerMode.EXTERNAL
                    ? Text.translatable("schemgen.status.server.external", session.serverVersion(),
                            session.config().externalUri().toString())
                    : Text.translatable("schemgen.status.server.sidecar", session.serverVersion());
            case FAILED -> Text.translatable("schemgen.status.server.failed").formatted(Formatting.RED);
            case CONNECTING, IDLE -> Text.translatable("schemgen.status.server.connecting");
        };
    }

    /** The progress bar and the status line; a status too long for its line shows whole on hover. */
    private void renderStatus(DrawContext context, int mouseX, int mouseY) {
        Converter converter = session.converter();
        int barWidth = width - 2 * PAD;
        context.fill(PAD, barY, PAD + barWidth, barY + 3, 0x60FFFFFF);
        int filled = (int) Math.round(barWidth * Math.max(0, Math.min(100, converter.progress())) / 100.0);
        if (filled > 0) {
            context.fill(PAD, barY, PAD + filled, barY + 3, converter.isRunning() ? 0xFF55AAFF : 0xFF55FF55);
        }
        List<OrderedText> lines = textRenderer.wrapLines(currentStatus(), barWidth);
        if (lines.isEmpty()) {
            return;
        }
        context.drawTextWithShadow(textRenderer, lines.get(0), PAD, statusY, WHITE);
        if (lines.size() > 1 && mouseY >= statusY && mouseY < statusY + textRenderer.fontHeight
                && mouseX >= PAD && mouseX < PAD + barWidth) {
            context.drawOrderedTooltip(textRenderer, textRenderer.wrapLines(currentStatus(), 300), mouseX, mouseY);
        }
    }

    private Text currentStatus() {
        Text conversion = session.converter().message();
        Text preview = session.preview().message();
        if (conversion != seenConversionMessage) {
            seenConversionMessage = conversion;
            status = conversion;
        }
        if (preview != seenPreviewMessage) {
            seenPreviewMessage = preview;
            if (!preview.getString().isEmpty()) {
                status = preview;
            }
        }
        return status;
    }

    @Override
    public void removed() {
        session.saveSettings();
    }
}
