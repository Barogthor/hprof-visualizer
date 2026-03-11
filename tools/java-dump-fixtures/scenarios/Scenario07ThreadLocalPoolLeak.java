import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.ThreadFactory;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;

public final class Scenario07ThreadLocalPoolLeak implements HeapScenario {
    @Override
    public String id() {
        return "07";
    }

    @Override
    public String name() {
        return "threadlocal-pool-leak";
    }

    @Override
    public ScenarioHandle setup(ProfileSpec spec) throws Exception {
        int workers = Math.max(2, Math.min(8, Runtime.getRuntime().availableProcessors()));
        ExecutorService pool = Executors.newFixedThreadPool(workers, new NamedThreadFactory("scenario-07-pool"));

        ThreadLocal<List<LeakEntry>> threadLocal = new ThreadLocal<>();
        CountDownLatch prepared = new CountDownLatch(workers);
        CountDownLatch release = new CountDownLatch(1);

        for (int i = 0; i < workers; i++) {
            final int workerId = i;
            pool.submit(() -> {
                List<LeakEntry> entries = new ArrayList<>();
                int perWorker = Math.max(512, spec.customCollectionSize / workers);
                for (int j = 0; j < perWorker; j++) {
                    entries.add(new LeakEntry(workerId, j, spec.seed));
                }

                threadLocal.set(entries);
                prepared.countDown();
                try {
                    release.await();
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                }
            });
        }

        boolean allPrepared = prepared.await(15, TimeUnit.SECONDS);
        if (!allPrepared) {
            throw new IllegalStateException("Thread-local pool did not initialize in time");
        }

        ThreadLocalRoot root = new ThreadLocalRoot();
        root.pool = pool;
        root.threadLocal = threadLocal;
        root.release = release;
        root.workerCount = workers;

        Map<String, String> metrics = new LinkedHashMap<>();
        metrics.put("scenario", "thread-local values retained on fixed pool threads");
        metrics.put("workerCount", Integer.toString(workers));
        metrics.put("perWorkerItems", Integer.toString(Math.max(512, spec.customCollectionSize / workers)));
        metrics.put("threadNamePrefix", "scenario-07-pool-");

        List<Object> roots = new ArrayList<>();
        roots.add(root);

        return new ScenarioHandle(roots, metrics, () -> {
            release.countDown();
            pool.shutdownNow();
        });
    }

    private static final class LeakEntry {
        private final int workerId;
        private final int index;
        private final String label;
        private final byte[] payload;

        private LeakEntry(int workerId, int index, int seed) {
            this.workerId = workerId;
            this.index = index;
            this.label = "tl-" + workerId + '-' + index + '-' + seed;
            this.payload = new byte[1024];
        }
    }

    private static final class ThreadLocalRoot {
        private ExecutorService pool;
        private ThreadLocal<List<LeakEntry>> threadLocal;
        private CountDownLatch release;
        private int workerCount;
    }

    private static final class NamedThreadFactory implements ThreadFactory {
        private final AtomicInteger sequence = new AtomicInteger();
        private final String prefix;

        private NamedThreadFactory(String prefix) {
            this.prefix = prefix;
        }

        @Override
        public Thread newThread(Runnable r) {
            Thread thread = new Thread(r, prefix + '-' + sequence.incrementAndGet());
            thread.setDaemon(true);
            return thread;
        }
    }
}
