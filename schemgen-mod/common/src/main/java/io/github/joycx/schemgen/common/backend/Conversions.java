package io.github.joycx.schemgen.common.backend;

import com.google.gson.JsonObject;
import io.github.joycx.schemgen.common.files.AtomicFiles;
import io.github.joycx.schemgen.common.model.Job;
import io.github.joycx.schemgen.common.model.StartedJobs;
import java.io.IOException;
import java.nio.file.Path;

/** One model through the server into a folder: upload, follow, download, thumbnail. */
public final class Conversions {
    private Conversions() {}

    /** Hears about a conversion: its job id once it exists (to cancel it), then every update. */
    public interface Listener extends JobListener {
        void started(String jobId);
    }

    /**
     * How a conversion ended. {@code schematic} and {@code thumbnail} are
     * {@code null} when it was cancelled; the thumbnail also when the server
     * had none.
     */
    public record Outcome(Job job, Path schematic, byte[] thumbnail) {
        public boolean cancelled() {
            return job.status() == Job.Status.CANCELLED;
        }
    }

    /**
     * Convert {@code model} and save the schematic in {@code outputFolder}
     * under the name the server gives it — {@code name-2.litematic} and so on
     * when that is taken, so nothing already there is overwritten.
     *
     * @throws BackendException when the conversion fails, with the server's reason
     */
    public static Outcome convert(BackendClient client, Path model, JsonObject settings, Path outputFolder,
            Listener listener) throws IOException, InterruptedException {
        StartedJobs started = client.startJob(model, settings);
        String id = started.jobId();
        if (id == null) {
            throw new BackendException("The server did not accept " + model.getFileName()
                    + (started.skipped() == null || started.skipped().isEmpty() ? "" : " (skipped: only .glb and .gltf convert)"));
        }
        listener.started(id);
        Job job = client.events(id, listener);
        return switch (job.status()) {
            case DONE -> {
                Path schematic = AtomicFiles.freeName(outputFolder, fileName(job));
                client.download(id, schematic);
                yield new Outcome(job, schematic, thumbnail(client, id));
            }
            case CANCELLED -> new Outcome(job, null, null);
            default -> throw new BackendException(job.error() == null || job.error().isBlank()
                    ? "The conversion failed" : job.error());
        };
    }

    /** The server's file name for the schematic, made safe for any file system. */
    static String fileName(Job job) {
        String name = job.downloadName() == null ? "" : job.downloadName().strip();
        name = name.replaceAll("[\\\\/:*?\"<>|\\p{Cntrl}]", "_");
        if (name.isEmpty() || name.equals(".litematic") || name.startsWith(".")) {
            name = "schematic.litematic";
        }
        return name.endsWith(".litematic") ? name : name + ".litematic";
    }

    /** The thumbnail is a nicety: a conversion that produced a file succeeded even without one. */
    private static byte[] thumbnail(BackendClient client, String id) throws InterruptedException {
        try {
            return client.thumbnail(id);
        } catch (IOException e) {
            return null;
        }
    }
}
