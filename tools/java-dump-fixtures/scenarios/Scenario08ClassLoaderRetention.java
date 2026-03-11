import java.net.URL;
import java.net.URLClassLoader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public final class Scenario08ClassLoaderRetention implements HeapScenario {
    private static final Map<String, Object> STATIC_PLUGIN_CACHE = new LinkedHashMap<>();

    @Override
    public String id() {
        return "08";
    }

    @Override
    public String name() {
        return "classloader-retention";
    }

    @Override
    public ScenarioHandle setup(ProfileSpec spec) throws Exception {
        Path tempDir = Files.createTempDirectory("scenario-08-loader-");
        Path configPath = tempDir.resolve("plugin-config.txt");
        String configText = "plugin.home=/opt/secret-plugin/" + spec.seed + "\n";
        Files.write(configPath, configText.getBytes(StandardCharsets.UTF_8));

        URLClassLoader loader = new URLClassLoader(new URL[]{tempDir.toUri().toURL()}, null);
        URL resource = loader.findResource("plugin-config.txt");

        PluginContext context = new PluginContext();
        context.name = "plugin-alpha";
        context.classLoader = loader;
        context.configUrl = resource;
        context.payload = new byte[Math.max(16 * 1024, spec.heavyBlockMiB * 1024 * 8)];
        context.cache = new LinkedHashMap<>();

        for (int i = 0; i < Math.max(256, spec.wrapperCollectionSize / 20); i++) {
            context.cache.put("entry-" + i, new PluginValue(i, spec.seed));
        }

        STATIC_PLUGIN_CACHE.put("active-plugin-context", context);
        STATIC_PLUGIN_CACHE.put("active-loader", loader);

        ClassLoaderRoot root = new ClassLoaderRoot();
        root.pluginContext = context;
        root.staticCache = STATIC_PLUGIN_CACHE;
        root.tempDirectory = tempDir;

        Map<String, String> metrics = new LinkedHashMap<>();
        metrics.put("scenario", "classloader retained through static cache");
        metrics.put("staticCacheEntries", Integer.toString(STATIC_PLUGIN_CACHE.size()));
        metrics.put("pluginCacheEntries", Integer.toString(context.cache.size()));
        metrics.put("tempDir", tempDir.toAbsolutePath().normalize().toString());

        List<Object> roots = new ArrayList<>();
        roots.add(root);

        return new ScenarioHandle(roots, metrics, () -> {
            STATIC_PLUGIN_CACHE.clear();
            loader.close();
            Files.deleteIfExists(configPath);
            Files.deleteIfExists(tempDir);
        });
    }

    private static final class PluginContext {
        private String name;
        private URLClassLoader classLoader;
        private URL configUrl;
        private byte[] payload;
        private Map<String, PluginValue> cache;
    }

    private static final class PluginValue {
        private final int id;
        private final String value;

        private PluginValue(int id, int seed) {
            this.id = id;
            this.value = "plugin-value-" + id + '-' + seed;
        }
    }

    private static final class ClassLoaderRoot {
        private PluginContext pluginContext;
        private Map<String, Object> staticCache;
        private Path tempDirectory;
    }
}
