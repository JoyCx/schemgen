package io.github.joycx.schemgen.common.model;

import com.google.gson.annotations.SerializedName;

/**
 * A conversion job as the server reports it: {@code GET /api/jobs/{id}}, each
 * item of {@code GET /api/jobs} and the data of every event.
 */
public record Job(
        String id,
        Status status,
        double progress,
        String stage,
        String message,
        @SerializedName("input_name") String inputName,
        String name,
        @SerializedName("download_name") String downloadName,
        String format,
        String target,
        @SerializedName("created_ms") long createdMs,
        @SerializedName("finished_ms") Long finishedMs,
        String error,
        JobResult result,
        @SerializedName("saved_path") String savedPath,
        @SerializedName("save_error") String saveError) {

    public enum Status {
        @SerializedName("queued")
        QUEUED,
        @SerializedName("running")
        RUNNING,
        @SerializedName("done")
        DONE,
        @SerializedName("error")
        ERROR,
        @SerializedName("cancelled")
        CANCELLED
    }

    /** Whether the job reached one of the three final states. */
    public boolean isFinished() {
        return status == Status.DONE || status == Status.ERROR || status == Status.CANCELLED;
    }
}
