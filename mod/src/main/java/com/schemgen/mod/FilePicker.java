package com.schemgen.mod;

import org.lwjgl.PointerBuffer;
import org.lwjgl.system.MemoryStack;
import org.lwjgl.util.tinyfd.TinyFileDialogs;

import java.nio.file.Path;
import java.util.function.Consumer;

/**
 * The native "open file" dialog, through LWJGL's TinyFileDialogs — already on
 * Minecraft's classpath, so this costs no dependency.
 *
 * <p>The dialog blocks until the user answers, so it must not run on the
 * render thread; {@link #choose} always hands it to a worker thread and calls
 * back from there. On a system with no dialog backend (a bare Linux box with
 * neither zenity nor kdialog) it simply returns nothing, which is why the GUI
 * also keeps a path text field.
 */
public final class FilePicker {

    private FilePicker() {}

    /**
     * Ask for a {@code .glb} / {@code .gltf} file.
     *
     * @param startIn   folder to open in, or empty for the OS default
     * @param onChosen  called off the client thread with the chosen path, or
     *                  {@code null} if the user cancelled
     */
    public static void choose(String startIn, Consumer<Path> onChosen) {
        Thread thread = new Thread(() -> {
            Path chosen = null;
            try {
                chosen = openDialog(startIn);
            } catch (Throwable t) {
                // A missing dialog backend throws rather than returning null.
                SchemGenMod.LOGGER.warn("No native file dialog available: {}", t.toString());
            }
            onChosen.accept(chosen);
        }, "schemgen-file-picker");
        thread.setDaemon(true);
        thread.start();
    }

    private static Path openDialog(String startIn) {
        try (MemoryStack stack = MemoryStack.stackPush()) {
            PointerBuffer filters = stack.mallocPointer(2);
            filters.put(stack.UTF8("*.glb"));
            filters.put(stack.UTF8("*.gltf"));
            filters.flip();

            String selected = TinyFileDialogs.tinyfd_openFileDialog(
                    "Select a model to convert",
                    startIn == null ? "" : startIn,
                    filters,
                    "glTF models (*.glb, *.gltf)",
                    false);

            return selected == null || selected.isBlank() ? null : Path.of(selected);
        }
    }
}
