package io.github.joycx.schemgen;

import com.google.gson.JsonObject;
import io.github.joycx.schemgen.common.backend.Conversions;
import io.github.joycx.schemgen.common.model.Job;
import io.github.joycx.schemgen.common.model.JobResult;
import io.github.joycx.schemgen.common.settings.SettingsModel;
import io.github.joycx.schemgen.litematica.LitematicaSupport;
import io.github.joycx.schemgen.ui.SchemGenScreen;
import io.github.joycx.schemgen.ui.Thumbnail;
import java.nio.file.Path;
import net.minecraft.client.MinecraftClient;
import net.minecraft.text.MutableText;
import net.minecraft.text.Text;
import net.minecraft.util.Formatting;

/**
 * The conversion the screen's Convert button starts: upload, progress, the
 * schematic saved into the output folder, its thumbnail, and — when set —
 * the hand-off to Litematica. It keeps running when the screen closes and
 * says in chat how it ended.
 */
public final class Converter {
    private final SchemGenSession session;
    private final Thumbnail thumbnail = new Thumbnail();

    private boolean running;
    private String jobId;
    private double progress;
    private Text message = Text.empty();
    private Path lastSchematic;
    private JobResult lastResult;

    Converter(SchemGenSession session) {
        this.session = session;
    }

    public void start() {
        Path model = session.selectedModel();
        SettingsModel settings = session.settings();
        if (running || model == null || settings == null) {
            return;
        }
        session.saveSettings();
        JsonObject request = settings.toJson();
        Path output = session.outputFolder();
        running = true;
        jobId = null;
        progress = 0;
        message = Text.translatable("schemgen.status.uploading", model.getFileName().toString());

        session.run(() -> {
            Conversions.Outcome outcome = Conversions.convert(session.backend().connect(), model, request, output,
                    new Conversions.Listener() {
                        @Override
                        public void started(String id) {
                            SchemGenSession.onClient(() -> jobId = id);
                        }

                        @Override
                        public void onUpdate(Job job) {
                            SchemGenSession.onClient(() -> progress(job));
                        }
                    });
            SchemGenSession.onClient(() -> finished(outcome));
        }, this::failed);
    }

    /** Ask the server to stop the running job; its event stream then ends as cancelled. */
    public void cancel() {
        String id = jobId;
        if (!running || id == null) {
            return;
        }
        message = Text.translatable("schemgen.status.cancelling");
        session.run(() -> session.backend().connect().cancel(id), this::failed);
    }

    /** Place the last schematic in Litematica, as the Load in Litematica button does. */
    public void loadLast() {
        if (lastSchematic != null && lastResult != null) {
            message = session.placeInLitematica(lastSchematic, lastResult.dims());
        }
    }

    private void progress(Job job) {
        if (!running) {
            return;
        }
        progress = job.progress();
        message = Text.literal(job.message() == null ? "" : job.message());
    }

    private void finished(Conversions.Outcome outcome) {
        running = false;
        jobId = null;
        if (outcome.cancelled()) {
            progress = 0;
            message = Text.translatable("schemgen.status.cancelled");
            return;
        }
        progress = 100;
        lastSchematic = outcome.schematic();
        lastResult = outcome.job().result();
        if (outcome.thumbnail() != null) {
            thumbnail.set(outcome.thumbnail());
            session.changed(); // the screen makes room for it
        }
        String name = lastSchematic.getFileName().toString();
        MutableText saved = Text.translatable("schemgen.status.saved", name, blocks(), dims());
        tellIfScreenClosed(Text.translatable("schemgen.chat.saved", name));
        if (session.config().autoLoadIntoLitematica && LitematicaSupport.isLoaded()
                && MinecraftClient.getInstance().player != null) {
            saved.append(" · ").append(session.placeInLitematica(lastSchematic, lastResult.dims()));
        }
        message = saved;
    }

    private void failed(String reason) {
        running = false;
        jobId = null;
        progress = 0;
        message = Text.translatable("schemgen.status.failed", reason).formatted(Formatting.RED);
        tellIfScreenClosed(Text.translatable("schemgen.chat.failed", reason).formatted(Formatting.RED));
    }

    /** A conversion outlives the screen; when it ends unseen, say so in chat. */
    private static void tellIfScreenClosed(Text text) {
        MinecraftClient client = MinecraftClient.getInstance();
        if (!(client.currentScreen instanceof SchemGenScreen) && client.player != null) {
            client.inGameHud.getChatHud().addMessage(text);
        }
    }

    private String blocks() {
        return lastResult == null ? "?" : Integer.toString(lastResult.blocks());
    }

    private String dims() {
        int[] d = lastResult == null ? null : lastResult.dims();
        return d == null || d.length != 3 ? "?" : d[0] + " × " + d[1] + " × " + d[2];
    }

    public boolean isRunning() {
        return running;
    }

    /** Whether the server has created the job yet — only then can it be cancelled. */
    public boolean canCancel() {
        return running && jobId != null;
    }

    /** 0–100. */
    public double progress() {
        return progress;
    }

    public Text message() {
        return message;
    }

    public Path lastSchematic() {
        return lastSchematic;
    }

    public JobResult lastResult() {
        return lastResult;
    }

    public Thumbnail thumbnail() {
        return thumbnail;
    }
}
