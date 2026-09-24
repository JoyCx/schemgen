package io.github.joycx.schemgen.common.backend;

import io.github.joycx.schemgen.common.model.Job;

/** Hears every state a followed job reports, the final one included. Called on the following thread. */
@FunctionalInterface
public interface JobListener {
    void onUpdate(Job job);
}
