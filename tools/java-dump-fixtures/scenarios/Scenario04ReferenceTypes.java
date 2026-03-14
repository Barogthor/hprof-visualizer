import java.lang.ref.PhantomReference;
import java.lang.ref.Reference;
import java.lang.ref.ReferenceQueue;
import java.lang.ref.SoftReference;
import java.lang.ref.WeakReference;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public final class Scenario04ReferenceTypes implements HeapScenario {
    @Override
    public String id() {
        return "04";
    }

    @Override
    public String name() {
        return "reference-types";
    }

    @Override
    public ScenarioHandle setup(ProfileSpec spec) {
        ReferenceRoot root = new ReferenceRoot();
        root.queue = new ReferenceQueue<>();
        root.strongRoots = new ArrayList<>();
        root.weakRefs = new ArrayList<>();
        root.softRefs = new ArrayList<>();
        root.phantomRefs = new ArrayList<>();
        root.polledQueue = new ArrayList<>();

        for (int i = 0; i < spec.customCollectionSize; i++) {
            RefPayload payload = new RefPayload(i, "weak-" + i);
            root.weakRefs.add(new WeakReference<>(payload, root.queue));
            if ((i & 15) == 0) {
                root.strongRoots.add(payload);
            }
        }

        for (int i = 0; i < spec.wrapperCollectionSize; i++) {
            RefPayload payload = new RefPayload(10_000 + i, "soft-" + i);
            root.softRefs.add(new SoftReference<>(payload, root.queue));
            if ((i & 31) == 0) {
                root.strongRoots.add(payload);
            }
        }

        for (int i = 0; i < Math.max(256, spec.objectArraySize / 2); i++) {
            RefPayload payload = new RefPayload(20_000 + i, "phantom-" + i);
            root.strongRoots.add(payload);
            root.phantomRefs.add(new PhantomReference<>(payload, root.queue));
        }

        System.gc();
        for (int i = 0; i < 128; i++) {
            Reference<?> polled = root.queue.poll();
            if (polled == null) {
                break;
            }
            root.polledQueue.add(polled);
        }

        Map<String, String> metrics = new LinkedHashMap<>();
        metrics.put("scenario", "weak + soft + phantom references");
        metrics.put("strongRoots", Integer.toString(root.strongRoots.size()));
        metrics.put("weakRefs", Integer.toString(root.weakRefs.size()));
        metrics.put("softRefs", Integer.toString(root.softRefs.size()));
        metrics.put("phantomRefs", Integer.toString(root.phantomRefs.size()));
        metrics.put("queuePolled", Integer.toString(root.polledQueue.size()));

        List<Object> roots = new ArrayList<>();
        roots.add(root);
        return new ScenarioHandle(roots, metrics, null);
    }

    private static final class RefPayload {
        private final int id;
        private final String label;
        private final byte[] bytes;

        private RefPayload(int id, String label) {
            this.id = id;
            this.label = label;
            this.bytes = new byte[1024];
        }
    }

    private static final class ReferenceRoot {
        private ReferenceQueue<Object> queue;
        private List<RefPayload> strongRoots;
        private List<WeakReference<RefPayload>> weakRefs;
        private List<SoftReference<RefPayload>> softRefs;
        private List<PhantomReference<RefPayload>> phantomRefs;
        private List<Reference<?>> polledQueue;
    }
}
