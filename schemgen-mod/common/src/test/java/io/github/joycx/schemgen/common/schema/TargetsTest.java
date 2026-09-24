package io.github.joycx.schemgen.common.schema;

import static org.junit.jupiter.api.Assertions.assertEquals;

import io.github.joycx.schemgen.common.Fixtures;
import java.util.List;
import org.junit.jupiter.api.Test;

class TargetsTest {
    private final List<Target> targets = Fixtures.schema().targets();

    @Test
    void aListedGameVersionIsItsOwnTarget() {
        assertEquals("1.21.8", Targets.forGame(targets, "1.21.8", 4440));
        assertEquals("1.21.1", Targets.forGame(targets, "1.21.1", 3955));
        assertEquals("26.1", Targets.forGame(targets, "26.1", 4786));
    }

    @Test
    void anUnlistedVersionWithAListedDataVersionUsesThatTarget() {
        // A pre-release or a renamed build that stamps a known data version.
        assertEquals("1.21.4", Targets.forGame(targets, "1.21.4-custom", 4189));
    }

    @Test
    void otherwiseTheBareDataVersionIsSent() {
        assertEquals("4554", Targets.forGame(targets, "1.21.9", 4554));
        assertEquals("5100", Targets.forGame(targets, "26.4", 5100));
        assertEquals("4440", Targets.forGame(List.of(), "1.21.8", 4440));
    }
}
