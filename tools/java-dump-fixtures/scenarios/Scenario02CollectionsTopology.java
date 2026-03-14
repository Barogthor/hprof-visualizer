import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.LinkedList;
import java.util.List;
import java.util.Map;
import java.util.Set;

public final class Scenario02CollectionsTopology implements HeapScenario {
    @Override
    public String id() {
        return "02";
    }

    @Override
    public String name() {
        return "collections-topology";
    }

    @Override
    public ScenarioHandle setup(ProfileSpec spec) {
        TopologyRoot root = new TopologyRoot();

        root.wrapperList = new ArrayList<>(spec.wrapperCollectionSize);
        for (int i = 0; i < spec.wrapperCollectionSize; i++) {
            root.wrapperList.add(Integer.valueOf(2_000_000 + (i * 29) + spec.seed));
        }

        root.objectArray = new Object[spec.objectArraySize];
        for (int i = 0; i < root.objectArray.length; i++) {
            root.objectArray[i] = (i % 3 == 0) ? null : new Payload(i, "arr-" + i);
        }

        root.linkedList = new LinkedList<>();
        for (int i = 0; i < spec.customCollectionSize; i++) {
            root.linkedList.add(new Payload(i, "linked-" + i));
        }

        root.deque = new ArrayDeque<>();
        for (int i = 0; i < spec.wrapperCollectionSize; i++) {
            root.deque.addLast(Long.valueOf(7_000_000_000L + ((long) i * 11L)));
        }

        root.collisionMap = new HashMap<>((spec.customCollectionSize * 4 / 3) + 1);
        for (int i = 0; i < spec.customCollectionSize; i++) {
            root.collisionMap.put(new CollisionKey(i), new Payload(i, "collision-" + i));
        }

        root.orderMap = new LinkedHashMap<>();
        root.orderSet = new LinkedHashSet<>();
        for (int i = 0; i < 64; i++) {
            String key = "order-" + i;
            root.orderMap.put(key, new Payload(i, key));
            root.orderSet.add(key);
        }

        Payload shared = new Payload(9_999_999, "shared-child");
        root.sharedReferences = new ArrayList<>();
        root.sharedReferences.add(shared);
        root.sharedReferences.add(shared);
        root.sharedReferences.add(shared);

        Map<String, String> metrics = new LinkedHashMap<>();
        metrics.put("scenario", "collections topologies + collisions + nulls");
        metrics.put("wrapperListSize", Integer.toString(root.wrapperList.size()));
        metrics.put("linkedListSize", Integer.toString(root.linkedList.size()));
        metrics.put("collisionMapSize", Integer.toString(root.collisionMap.size()));
        metrics.put("sharedRefCopies", Integer.toString(root.sharedReferences.size()));

        List<Object> roots = new ArrayList<>();
        roots.add(root);
        return new ScenarioHandle(roots, metrics, null);
    }

    private static final class TopologyRoot {
        private List<Integer> wrapperList;
        private Object[] objectArray;
        private LinkedList<Payload> linkedList;
        private ArrayDeque<Long> deque;
        private Map<CollisionKey, Payload> collisionMap;
        private Map<String, Payload> orderMap;
        private Set<String> orderSet;
        private List<Payload> sharedReferences;
    }

    private static final class Payload {
        private final int id;
        private final String label;

        private Payload(int id, String label) {
            this.id = id;
            this.label = label;
        }
    }

    private static final class CollisionKey {
        private final int value;

        private CollisionKey(int value) {
            this.value = value;
        }

        @Override
        public int hashCode() {
            return 42;
        }

        @Override
        public boolean equals(Object other) {
            if (!(other instanceof CollisionKey)) {
                return false;
            }
            return this.value == ((CollisionKey) other).value;
        }
    }
}
