package com.schemgen.mod;

import java.nio.file.Path;
import java.util.concurrent.atomic.AtomicBoolean;

/**
 * One conversion, running on its own thread.
 *
 * <p>Upload, polling and the download all block, so none of it may touch the
 * client thread. The GUI reads {@link #stage()}, {@link #percent()} and
 * {@link #message()} every frame instead — they are volatile, written only
 * here and read only there.
 */
public final class ConversionTask {

    public enum Stage {
        /** Checking that a server is there. */
        CONNECTING,
        /** Sending the model. */
        UPLOADING,
        /** Server is voxelizing, matching and writing. */
        CONVERTING,
        /** Fetching the finished file and writing it to schematics/. */
        SAVING,
        DONE,
        FAILED
    }

    private static final long POLL_INTERVAL_MS = 400;

    private final ApiClient api = new ApiClient();
    private final AtomicBoolean cancelled = new AtomicBoolean();

    private final String serverUrl;
    private final Path model;
    private final ApiClient.Settings settings;

    private volatile Stage stage = Stage.CONNECTING;
    private volatile float percent;
    private volatile String message = "Contacting the server...";
    private volatile Path savedTo;

    private Thread worker;

    public ConversionTask(String serverUrl, Path model, ApiClient.Settings settings) {
        this.serverUrl = serverUrl;
        this.model = model;
        this.settings = settings;
    }

    public Stage stage() {
        return stage;
    }

    /** 0..1 across the whole job, not just the server-side part. */
    public float percent() {
        return percent;
    }

    public String message() {
        return message;
    }

    /** Set once {@link Stage#DONE} is reached. */
    public Path savedTo() {
        return savedTo;
    }

    public boolean finished() {
        return stage == Stage.DONE || stage == Stage.FAILED;
    }

    public void start() {
        if (worker != null) {
            throw new IllegalStateException("This task was already started");
        }
        worker = new Thread(this::run, "schemgen-conversion");
        worker.setDaemon(true);
        worker.start();
    }

    /**
     * Stop reporting and stop polling. The server keeps finishing the job it
     * already started — there is no cancel route — but nothing is saved.
     */
    public void cancel() {
        cancelled.set(true);
        if (worker != null) {
            worker.interrupt();
        }
    }

    private void run() {
        try {
            ApiClient.Health health = api.health(serverUrl);
            report(Stage.UPLOADING, 0.02f,
                    "Uploading " + model.getFileName() + " to SchemGen2 " + health.version() + "...");

            String jobId = api.startConversion(serverUrl, model, settings);
            report(Stage.CONVERTING, 0.05f, "Converting...");

            while (!cancelled.get()) {
                Thread.sleep(POLL_INTERVAL_MS);
                ApiClient.Progress progress = api.progress(serverUrl, jobId);
                if (progress.failed()) {
                    fail(progress.message().isBlank() ? "The conversion failed" : progress.message());
                    return;
                }
                if (progress.done()) {
                    break;
                }
                // The server reports 0..100 for its own share of the work; keep
                // the last tenth for downloading and saving.
                report(Stage.CONVERTING,
                        0.05f + Math.clamp(progress.percent() / 100.0f, 0.0f, 1.0f) * 0.85f,
                        progress.message());
            }
            if (cancelled.get()) {
                return;
            }

            report(Stage.SAVING, 0.92f, "Saving to the schematics folder...");
            byte[] schematic = api.download(serverUrl, jobId);
            savedTo = SchematicsFolder.save(settings.schematicName(), schematic);

            percent = 1.0f;
            message = "Saved " + savedTo.getFileName() + " — open it in Litematica";
            stage = Stage.DONE;
            SchemGenMod.LOGGER.info("Saved {} ({} bytes)", savedTo, schematic.length);

        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            // Only ever from cancel(); leave the stage as it was.
        } catch (Exception e) {
            fail(e.getMessage() == null ? e.toString() : e.getMessage());
        }
    }

    private void report(Stage newStage, float newPercent, String newMessage) {
        if (cancelled.get()) {
            return;
        }
        stage = newStage;
        percent = newPercent;
        message = newMessage;
    }

    private void fail(String why) {
        if (cancelled.get()) {
            return;
        }
        stage = Stage.FAILED;
        message = why;
        SchemGenMod.LOGGER.warn("Conversion failed: {}", why);
    }
}
