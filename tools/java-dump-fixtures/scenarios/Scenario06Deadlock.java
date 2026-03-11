import java.lang.management.ManagementFactory;
import java.lang.management.ThreadMXBean;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;

public final class Scenario06Deadlock implements HeapScenario {
    @Override
    public String id() {
        return "06";
    }

    @Override
    public String name() {
        return "thread-deadlock";
    }

    @Override
    public ScenarioHandle setup(ProfileSpec spec) throws Exception {
        DeadlockRoot root = new DeadlockRoot();
        root.lockA = new Object();
        root.lockB = new Object();
        root.firstLockReady = new CountDownLatch(2);

        DeadlockWorker workerA = new DeadlockWorker(
                "scenario-06-deadlock-A",
                root.lockA,
                root.lockB,
                root.firstLockReady
        );
        DeadlockWorker workerB = new DeadlockWorker(
                "scenario-06-deadlock-B",
                root.lockB,
                root.lockA,
                root.firstLockReady
        );

        workerA.start();
        workerB.start();
        root.threads = List.of(workerA.thread, workerB.thread);

        boolean ready = root.firstLockReady.await(10, TimeUnit.SECONDS);
        if (!ready) {
            throw new IllegalStateException("Deadlock workers did not acquire first lock in time");
        }

        ThreadMXBean bean = ManagementFactory.getThreadMXBean();
        long[] ids = waitForDeadlock(bean, 50, 100);
        if (ids == null || ids.length < 2) {
            throw new IllegalStateException("Expected deadlock was not detected");
        }

        root.detectedDeadlockedThreadIds = ids;

        Map<String, String> metrics = new LinkedHashMap<>();
        metrics.put("scenario", "intentional monitor deadlock with two threads");
        metrics.put("deadlockedThreadCount", Integer.toString(ids.length));
        metrics.put("deadlockedThreadIds", Arrays.toString(ids));
        metrics.put("threadA", workerA.thread.getName());
        metrics.put("threadB", workerB.thread.getName());

        List<Object> roots = new ArrayList<>();
        roots.add(root);
        return new ScenarioHandle(roots, metrics, null);
    }

    private static long[] waitForDeadlock(ThreadMXBean bean, int maxAttempts, long sleepMillis) throws InterruptedException {
        for (int i = 0; i < maxAttempts; i++) {
            long[] deadlocked = bean.findDeadlockedThreads();
            if (deadlocked != null && deadlocked.length > 0) {
                return deadlocked;
            }
            Thread.sleep(sleepMillis);
        }
        return null;
    }

    private static final class DeadlockWorker implements Runnable {
        private final Object first;
        private final Object second;
        private final CountDownLatch firstLockReady;
        private final Thread thread;

        private DeadlockWorker(String name, Object first, Object second, CountDownLatch firstLockReady) {
            this.first = first;
            this.second = second;
            this.firstLockReady = firstLockReady;
            this.thread = new Thread(this, name);
            this.thread.setDaemon(true);
        }

        private void start() {
            this.thread.start();
        }

        @Override
        public void run() {
            synchronized (first) {
                firstLockReady.countDown();
                try {
                    firstLockReady.await();
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                    return;
                }
                synchronized (second) {
                    FixtureRuntime.touch(second);
                }
            }
        }
    }

    private static final class DeadlockRoot {
        private Object lockA;
        private Object lockB;
        private CountDownLatch firstLockReady;
        private List<Thread> threads;
        private long[] detectedDeadlockedThreadIds;
    }
}
