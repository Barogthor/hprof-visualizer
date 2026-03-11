import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.locks.LockSupport;

public final class Scenario03LeakPatterns implements HeapScenario {
    @Override
    public String id() {
        return "03";
    }

    @Override
    public String name() {
        return "leak-patterns";
    }

    @Override
    public ScenarioHandle setup(ProfileSpec spec) throws Exception {
        LeakRegistry.reset();

        for (int i = 0; i < spec.wrapperCollectionSize; i++) {
            LeakRegistry.STATIC_CACHE.put("cache-" + i, new LeakValue(i, spec.seed));
        }

        LeakyWorker worker = new LeakyWorker(spec);
        worker.startAndAwaitReady();

        LeakRoot root = new LeakRoot();
        root.classLoaderLike = new PseudoClassLoader("plugin-loader-A");
        root.classLoaderLike.registry = LeakRegistry.STATIC_CACHE;
        root.classLoaderLike.pluginPayload = new byte[spec.heavyBlockMiB * 1024 * 256];

        Map<String, String> metrics = new LinkedHashMap<>();
        metrics.put("scenario", "static cache + thread local + classloader-like graph");
        metrics.put("staticCacheSize", Integer.toString(LeakRegistry.STATIC_CACHE.size()));
        metrics.put("threadLocalPayloadItems", Integer.toString(spec.customCollectionSize));
        metrics.put("workerThreadName", worker.threadName());

        List<Object> roots = new ArrayList<>();
        roots.add(root);

        return new ScenarioHandle(roots, metrics, () -> {
            worker.stopAndJoin();
            LeakRegistry.reset();
        });
    }

    private static final class LeakRegistry {
        private static final Map<String, LeakValue> STATIC_CACHE = new LinkedHashMap<>();

        private LeakRegistry() {
        }

        private static void reset() {
            STATIC_CACHE.clear();
        }
    }

    private static final class LeakValue {
        private final int id;
        private final String value;
        private final byte[] payload;

        private LeakValue(int id, int seed) {
            this.id = id;
            this.value = "leak-" + id + "-" + seed;
            this.payload = new byte[2048];
            for (int i = 0; i < this.payload.length; i++) {
                this.payload[i] = (byte) ((i + id) & 0xFF);
            }
        }
    }

    private static final class PseudoClassLoader {
        private final String loaderName;
        private Map<String, LeakValue> registry;
        private byte[] pluginPayload;

        private PseudoClassLoader(String loaderName) {
            this.loaderName = loaderName;
        }
    }

    private static final class LeakRoot {
        private PseudoClassLoader classLoaderLike;
    }

    private static final class LeakyWorker {
        private final CountDownLatch ready = new CountDownLatch(1);
        private final Thread thread;
        private volatile boolean running = true;

        private LeakyWorker(ProfileSpec spec) {
            this.thread = new Thread(() -> run(spec), "scenario-03-threadlocal-worker");
        }

        void startAndAwaitReady() throws InterruptedException {
            thread.start();
            ready.await();
        }

        void stopAndJoin() throws InterruptedException {
            running = false;
            thread.join();
        }

        String threadName() {
            return thread.getName();
        }

        private void run(ProfileSpec spec) {
            ThreadLocal<List<LeakValue>> threadLocal = new ThreadLocal<>();
            List<LeakValue> values = new ArrayList<>(spec.customCollectionSize);
            for (int i = 0; i < spec.customCollectionSize; i++) {
                values.add(new LeakValue(100_000 + i, spec.seed));
            }
            threadLocal.set(values);

            ready.countDown();

            long tick = 0L;
            while (running) {
                int idx = (int) (tick % values.size());
                FixtureRuntime.touch(values.get(idx));
                LockSupport.parkNanos(2_000_000L);
                tick++;
            }

            FixtureRuntime.touch(values.get(0));
        }
    }
}
