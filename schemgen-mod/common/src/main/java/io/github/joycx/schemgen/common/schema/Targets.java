package io.github.joycx.schemgen.common.schema;

import java.util.List;

/** Which target a conversion made from inside a game should use. */
public final class Targets {
    private Targets() {}

    /**
     * The target for the running game: its version when the server lists it,
     * else the listed version with the same data version, else the bare data
     * version as a string — which the server accepts as a target of its own
     * (a game newer than the server's table still gets a correct stamp).
     */
    public static String forGame(List<Target> targets, String gameVersion, int dataVersion) {
        for (Target t : targets) {
            if (t.id().equals(gameVersion)) {
                return t.id();
            }
        }
        for (Target t : targets) {
            if (t.dataVersion() == dataVersion) {
                return t.id();
            }
        }
        return Integer.toString(dataVersion);
    }
}
