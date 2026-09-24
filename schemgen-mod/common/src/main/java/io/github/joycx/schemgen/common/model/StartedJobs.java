package io.github.joycx.schemgen.common.model;

import com.google.gson.annotations.SerializedName;
import java.util.List;

/** The answer to {@code POST /api/jobs}. */
public record StartedJobs(
        @SerializedName("job_id") String jobId,
        List<Entry> jobs,
        List<String> skipped,
        List<String> ignored,
        @SerializedName("output_dir") String outputDir) {

    public record Entry(@SerializedName("job_id") String jobId, String filename, String name) {}
}
