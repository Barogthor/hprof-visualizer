import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

public final class Scenario09ConcurrentMapHotBuckets implements HeapScenario {
    @Override
    public String id() {
        return "09";
    }

    @Override
    public String name() {
        return "concurrent-map-hot-buckets";
    }

    @Override
    public ScenarioHandle setup(ProfileSpec spec) {
        ConcurrentMapRoot root = new ConcurrentMapRoot();
        root.hotBuckets = new ConcurrentHashMap<>();
        root.lookupKeys = new ArrayList<>();
        root.sharedPayloads = new ArrayList<>();

        int entries = Math.max(20_000, spec.wrapperCollectionSize);
        int sharedGroup = 128;
        for (int i = 0; i < entries; i++) {
            HotValue value;
            if (i % sharedGroup == 0) {
                value = new HotValue(i, "shared-" + i, true);
                root.sharedPayloads.add(value);
            } else {
                HotValue shared = root.sharedPayloads.get(i % root.sharedPayloads.size());
                value = new HotValue(i, shared.tag, false);
                value.link = shared;
            }

            CollisionKey key = new CollisionKey(i);
            root.hotBuckets.put(key, value);
            if ((i & 31) == 0) {
                root.lookupKeys.add(key);
            }
        }

        root.nullValueMap = new ConcurrentHashMap<>();
        for (int i = 0; i < 512; i++) {
            root.nullValueMap.put("key-" + i, (i % 5 == 0) ? "" : "value-" + i);
        }

        Map<String, String> metrics = new LinkedHashMap<>();
        metrics.put("scenario", "concurrent hash map with heavy collisions");
        metrics.put("entryCount", Integer.toString(root.hotBuckets.size()));
        metrics.put("lookupKeys", Integer.toString(root.lookupKeys.size()));
        metrics.put("sharedPayloads", Integer.toString(root.sharedPayloads.size()));
        metrics.put("forcedHashCode", "17");

        List<Object> roots = new ArrayList<>();
        roots.add(root);
        return new ScenarioHandle(roots, metrics, null);
    }

    private static final class CollisionKey {
        private final int id;

        private CollisionKey(int id) {
            this.id = id;
        }

        @Override
        public int hashCode() {
            return 17;
        }

        @Override
        public boolean equals(Object other) {
            if (!(other instanceof CollisionKey)) {
                return false;
            }
            return this.id == ((CollisionKey) other).id;
        }
    }

    private static final class HotValue {
        private final int id;
        private final String tag;
        private final boolean anchor;
        private final byte[] payload;
        private HotValue link;

        private HotValue(int id, String tag, boolean anchor) {
            this.id = id;
            this.tag = tag;
            this.anchor = anchor;
            this.payload = new byte[512];
        }
    }

    private static final class ConcurrentMapRoot {
        private ConcurrentHashMap<CollisionKey, HotValue> hotBuckets;
        private List<CollisionKey> lookupKeys;
        private List<HotValue> sharedPayloads;
        private ConcurrentHashMap<String, String> nullValueMap;
    }
}
