import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public final class ScenarioHandle implements AutoCloseable {
    private final List<Object> roots;
    private final Map<String, String> metrics;
    private final AutoCloseable cleanup;

    public ScenarioHandle(List<Object> roots, Map<String, String> metrics, AutoCloseable cleanup) {
        this.roots = roots;
        this.metrics = metrics == null ? Collections.emptyMap() : new LinkedHashMap<>(metrics);
        this.cleanup = cleanup;
    }

    public List<Object> roots() {
        return roots;
    }

    public Map<String, String> metrics() {
        return metrics;
    }

    @Override
    public void close() throws Exception {
        if (cleanup != null) {
            cleanup.close();
        }
    }
}
