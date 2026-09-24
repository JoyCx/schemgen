package io.github.joycx.schemgen.ui;

import io.github.joycx.schemgen.SchemGenSession;
import io.github.joycx.schemgen.common.files.ModelFiles;
import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.InvalidPathException;
import java.nio.file.Path;
import java.util.List;
import java.util.function.Consumer;
import net.minecraft.text.Text;
import org.lwjgl.PointerBuffer;
import org.lwjgl.system.MemoryStack;
import org.lwjgl.util.tinyfd.TinyFileDialogs;

/**
 * Where models come from: the models folder, listed in the screen, and the
 * system's own file dialog through LWJGL's tinyfd, which Minecraft ships.
 */
public final class FilePicker {
    private FilePicker() {}

    /** The models in {@code folder}, which is created first so players find where to put them. */
    public static List<Path> list(Path folder) throws IOException {
        Files.createDirectories(folder);
        return ModelFiles.list(folder);
    }

    /**
     * Open the system's file dialog for a glTF model. The dialog blocks until
     * the player chooses, so it runs on a thread of its own; {@code onChosen}
     * gets the file on the client thread, and is not called on cancel.
     */
    public static void browse(Path startFolder, Consumer<Path> onChosen) {
        String title = Text.translatable("schemgen.models.browse.title").getString();
        String filterName = Text.translatable("schemgen.models.browse.filter").getString();
        Thread thread = new Thread(() -> {
            String chosen;
            try (MemoryStack stack = MemoryStack.stackPush()) {
                PointerBuffer patterns = stack.mallocPointer(2);
                patterns.put(stack.UTF8("*.glb"));
                patterns.put(stack.UTF8("*.gltf"));
                patterns.flip();
                chosen = TinyFileDialogs.tinyfd_openFileDialog(title, startFolder + File.separator, patterns, filterName, false);
            }
            if (chosen != null) {
                try {
                    Path file = Path.of(chosen);
                    SchemGenSession.onClient(() -> onChosen.accept(file));
                } catch (InvalidPathException ignored) {
                    // Nothing sensible to open.
                }
            }
        }, "SchemGen file dialog");
        thread.setDaemon(true);
        thread.start();
    }
}
