package com.schemgen.mod;

import com.mojang.brigadier.arguments.StringArgumentType;
import com.mojang.brigadier.context.CommandContext;

import net.fabricmc.fabric.api.client.command.v2.ClientCommandManager;
import net.fabricmc.fabric.api.client.command.v2.ClientCommandRegistrationCallback;
import net.fabricmc.fabric.api.client.command.v2.FabricClientCommandSource;
import net.minecraft.client.MinecraftClient;
import net.minecraft.text.Text;
import net.minecraft.util.Formatting;
import net.minecraft.util.Util;

import java.nio.file.Files;
import java.nio.file.Path;

/**
 * The {@code /schemgen} client command — the same actions as the GUI, for
 * people who would rather type, and the only way to convert without opening a
 * screen.
 *
 * <p>It is a <em>client</em> command: it never reaches the server you are
 * playing on, and works in singleplayer, on multiplayer and on the title
 * screen alike.
 *
 * <pre>
 * /schemgen                    open the GUI
 * /schemgen convert &lt;path&gt;      convert a model with the saved settings
 * /schemgen status             check that the API server is up
 * /schemgen server &lt;url&gt;        point the mod at another server
 * /schemgen folder             open the schematics folder
 * </pre>
 */
public final class SchemGenCommands {

    private SchemGenCommands() {}

    public static void register() {
        ClientCommandRegistrationCallback.EVENT.register((dispatcher, registryAccess) ->
                dispatcher.register(ClientCommandManager.literal("schemgen")
                        .executes(SchemGenCommands::openGui)
                        .then(ClientCommandManager.literal("convert")
                                .then(ClientCommandManager.argument("path", StringArgumentType.greedyString())
                                        .executes(SchemGenCommands::convert)))
                        .then(ClientCommandManager.literal("status")
                                .executes(SchemGenCommands::status))
                        .then(ClientCommandManager.literal("server")
                                .then(ClientCommandManager.argument("url", StringArgumentType.greedyString())
                                        .executes(SchemGenCommands::setServer)))
                        .then(ClientCommandManager.literal("folder")
                                .executes(SchemGenCommands::openFolder))));
    }

    private static int openGui(CommandContext<FabricClientCommandSource> context) {
        SchemGenMod.openScreen();
        return 1;
    }

    private static int convert(CommandContext<FabricClientCommandSource> context) {
        String raw = StringArgumentType.getString(context, "path").trim().replaceAll("^\"|\"$", "");
        FabricClientCommandSource source = context.getSource();

        String lower = raw.toLowerCase();
        if (!lower.endsWith(".glb") && !lower.endsWith(".gltf")) {
            source.sendError(Text.literal("Only .glb and .gltf models can be converted"));
            return 0;
        }

        Path model = Path.of(raw);
        if (!Files.isRegularFile(model)) {
            source.sendError(Text.literal("No file at " + raw));
            return 0;
        }

        SchemGenConfig config = SchemGenMod.config();
        String name = SchematicsFolder.sanitize(stripExtension(model.getFileName().toString()));
        ConversionTask task = new ConversionTask(config.serverUrl, model,
                new ApiClient.Settings(config.maxSize, config.dither, config.delight, name));

        source.sendFeedback(Text.literal("Converting " + model.getFileName()
                + " at " + config.maxSize + " blocks...").formatted(Formatting.GRAY));
        task.start();
        watchInChat(task);
        return 1;
    }

    private static int status(CommandContext<FabricClientCommandSource> context) {
        FabricClientCommandSource source = context.getSource();
        String url = SchemGenMod.config().serverUrl;

        // Even a refused connection takes a moment; keep it off the client thread.
        Thread probe = new Thread(() -> {
            try {
                ApiClient.Health health = new ApiClient().health(url);
                chat(Text.literal("SchemGen2 " + health.version() + " up at " + url
                                + " — " + health.paletteBlocks() + " blocks, data version "
                                + health.dataVersion())
                        .formatted(Formatting.GREEN));
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
            } catch (Exception e) {
                chat(Text.literal(e.getMessage()).formatted(Formatting.RED));
            }
        }, "schemgen-status");
        probe.setDaemon(true);
        probe.start();

        source.sendFeedback(Text.literal("Pinging " + url + "...").formatted(Formatting.GRAY));
        return 1;
    }

    private static int setServer(CommandContext<FabricClientCommandSource> context) {
        String url = StringArgumentType.getString(context, "url").trim();
        if (!url.startsWith("http://") && !url.startsWith("https://")) {
            context.getSource().sendError(Text.literal("The URL must start with http:// or https://"));
            return 0;
        }
        SchemGenConfig config = SchemGenMod.config();
        config.serverUrl = url;
        config.save();
        context.getSource().sendFeedback(
                Text.literal("SchemGen2 server set to " + config.serverUrl).formatted(Formatting.GREEN));
        return 1;
    }

    private static int openFolder(CommandContext<FabricClientCommandSource> context) {
        Path folder = SchematicsFolder.path();
        try {
            Files.createDirectories(folder);
            Util.getOperatingSystem().open(folder.toUri());
            context.getSource().sendFeedback(Text.literal(folder.toString()).formatted(Formatting.GRAY));
            return 1;
        } catch (Exception e) {
            context.getSource().sendError(Text.literal("Could not open " + folder + ": " + e.getMessage()));
            return 0;
        }
    }

    /** Poll a running task and report only its ending in chat. */
    private static void watchInChat(ConversionTask task) {
        Thread watcher = new Thread(() -> {
            try {
                while (!task.finished()) {
                    Thread.sleep(500);
                }
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                return;
            }
            boolean ok = task.stage() == ConversionTask.Stage.DONE;
            chat(Text.literal(task.message()).formatted(ok ? Formatting.GREEN : Formatting.RED));
        }, "schemgen-chat-watcher");
        watcher.setDaemon(true);
        watcher.start();
    }

    /** Post a client-side chat line from any thread. */
    private static void chat(Text text) {
        SchemGenMod.onClientThread(() -> {
            MinecraftClient client = MinecraftClient.getInstance();
            if (client.inGameHud != null) {
                client.inGameHud.getChatHud().addMessage(text);
            } else {
                SchemGenMod.LOGGER.info("{}", text.getString());
            }
        });
    }

    private static String stripExtension(String fileName) {
        int dot = fileName.lastIndexOf('.');
        return dot > 0 ? fileName.substring(0, dot) : fileName;
    }
}
