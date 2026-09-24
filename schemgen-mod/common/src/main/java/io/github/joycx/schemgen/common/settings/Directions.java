package io.github.joycx.schemgen.common.settings;

/**
 * Direction vectors as two angles, the way both UIs show the key light.
 *
 * <p>Model space is glTF's: Y up, right-handed. Azimuth turns around +Y from
 * +Z toward +X; elevation is the angle above the XZ plane. So
 * {@code x = cos(el)·sin(az)}, {@code y = sin(el)}, {@code z = cos(el)·cos(az)}
 * — the same formulas as the web UI's {@code lighting.js}.
 */
public final class Directions {
    private Directions() {}

    public static double[] fromAngles(double azimuthDeg, double elevationDeg) {
        double az = Math.toRadians(azimuthDeg);
        double el = Math.toRadians(elevationDeg);
        double flat = Math.cos(el);
        return new double[] {flat * Math.sin(az), Math.sin(el), flat * Math.cos(az)};
    }

    /** Azimuth of {@code v} in degrees, in (-180, 180]. */
    public static double azimuth(double[] v) {
        return Math.toDegrees(Math.atan2(v[0], v[2]));
    }

    /** Elevation of {@code v} in degrees, in [-90, 90]. */
    public static double elevation(double[] v) {
        double length = length(v);
        if (length == 0) {
            return 0;
        }
        return Math.toDegrees(Math.asin(Math.max(-1, Math.min(1, v[1] / length))));
    }

    /** {@code v} scaled to length 1, or {@code null} when it has no direction. */
    public static double[] normalize(double[] v) {
        double length = length(v);
        if (!(length > 1e-12) || !Double.isFinite(length)) {
            return null;
        }
        return new double[] {v[0] / length, v[1] / length, v[2] / length};
    }

    static double length(double[] v) {
        return Math.sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
    }
}
