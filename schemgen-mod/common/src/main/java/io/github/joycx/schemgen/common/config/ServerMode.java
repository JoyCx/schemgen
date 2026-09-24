package io.github.joycx.schemgen.common.config;

/** Where the mod's conversions run. */
public enum ServerMode {
    /** The mod starts its own schemgen2 in the background and stops it with the game. */
    SIDECAR,
    /** A schemgen2 the player runs themselves, at a host and port. */
    EXTERNAL
}
