import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Deque;
import java.util.HashMap;
import java.util.HashSet;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.LinkedList;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.locks.LockSupport;

public final class Scenario01StackFrameTypes implements HeapScenario {
    @Override
    public String id() {
        return "01";
    }

    @Override
    public String name() {
        return "stack-frame-types";
    }

    @Override
    public ScenarioHandle setup(ProfileSpec spec) throws Exception {
        FrameRootWorker frameRootWorker = new FrameRootWorker(spec.frameRootObjectCount);
        frameRootWorker.startAndAwaitReady();

        try {
            Universe universe = buildUniverse(spec);
            validateNoUnexpectedAliasing(universe);
            long checksum = computeChecksum(universe);

            Map<String, String> metrics = new LinkedHashMap<>();
            metrics.put("scenario", "stack + java types via locals");
            metrics.put("checksum", "0x" + Long.toHexString(checksum));
            metrics.put("wrapperCollectionSize", Integer.toString(universe.wrapperInts.size()));
            metrics.put("customCollectionSize", Integer.toString(universe.customLinkedList.size()));
            metrics.put("hugeCollectionSize", Integer.toString(universe.hugeWrapperLongs.size()));
            metrics.put("matrixRows", Integer.toString(universe.primitiveArrays.intMatrix.length));
            metrics.put("matrixCols", Integer.toString(universe.primitiveArrays.intMatrix[0].length));

            List<Object> roots = new ArrayList<>();
            roots.add(universe);

            return new ScenarioHandle(roots, metrics, frameRootWorker::stopAndJoin);
        } catch (Exception setupError) {
            frameRootWorker.stopAndJoin();
            throw setupError;
        }
    }

    private static Universe buildUniverse(ProfileSpec spec) {
        PrimitiveBox primitiveBox = new PrimitiveBox(
                true,
                (byte) 0x5A,
                (short) 32_001,
                1_234_567_890,
                9_876_543_210_123L,
                'J',
                3.14159f,
                2.718281828
        );

        PrimitiveArrays primitiveArrays = createPrimitiveArrays(spec);
        CustomWithoutStatic[] customArray = createCustomArray(spec.customArraySize, spec.seed + 11);
        Object[] mixedObjectArray = createMixedObjectArray(spec.objectArraySize, customArray, spec.seed + 23);

        List<Integer> wrapperInts = createWrapperIntegerList(spec.wrapperCollectionSize, spec.seed + 31);
        LinkedList<CustomWithoutStatic> customLinkedList = createCustomLinkedList(spec.customCollectionSize, spec.seed + 37);
        Deque<Long> wrapperLongDeque = createWrapperLongDeque(spec.wrapperCollectionSize, spec.seed + 41);
        Set<String> wrapperStringSet = createStringSet(spec.wrapperCollectionSize, spec.seed + 43);
        Map<String, CustomWithoutStatic> customMap = createCustomMap(spec.customCollectionSize, spec.seed + 47);

        List<Long> hugeWrapperLongs = spec.hugeCollectionSize > 0
                ? createHugeWrapperLongList(spec.hugeCollectionSize, spec.seed + 53)
                : Collections.emptyList();

        CustomWithStatic staticRoot = createStaticCustomGraph(spec.customGraphNodeCount, spec.seed + 59);
        CustomWithoutStatic plainRoot = createPlainCustomChain(spec.customGraphNodeCount, spec.seed + 61);

        DirectCycle directCycle = createDirectCycle(spec.seed + 67);
        IndirectCycle2NodeA indirectCycle2 = createIndirectCycle2(spec.seed + 71);
        IndirectCycle3NodeA indirectCycle3 = createIndirectCycle3(spec.seed + 73);

        return new Universe(
                spec,
                primitiveBox,
                primitiveArrays,
                mixedObjectArray,
                customArray,
                wrapperInts,
                customLinkedList,
                wrapperLongDeque,
                wrapperStringSet,
                customMap,
                hugeWrapperLongs,
                staticRoot,
                plainRoot,
                SampleEnum.GAMMA,
                directCycle,
                indirectCycle2,
                indirectCycle3
        );
    }

    private static PrimitiveArrays createPrimitiveArrays(ProfileSpec spec) {
        int primitiveLen = Math.max(256, spec.objectArraySize);
        boolean[] bools = new boolean[primitiveLen];
        byte[] bytes = new byte[primitiveLen];
        short[] shorts = new short[primitiveLen];
        int[] ints = new int[primitiveLen];
        long[] longs = new long[primitiveLen];
        char[] chars = new char[primitiveLen];
        float[] floats = new float[primitiveLen];
        double[] doubles = new double[primitiveLen];

        for (int i = 0; i < primitiveLen; i++) {
            bools[i] = (i & 1) == 0;
            bytes[i] = (byte) (i & 0x7F);
            shorts[i] = (short) (1_000 + i);
            ints[i] = 100_000 + (i * 17);
            longs[i] = 1_000_000_000L + ((long) i * 131L);
            chars[i] = (char) ('A' + (i % 26));
            floats[i] = (float) (i * 0.5 + 0.25);
            doubles[i] = i * 1.25 + 0.125;
        }

        int[][] intMatrix = new int[spec.matrixRows][spec.matrixCols];
        for (int row = 0; row < spec.matrixRows; row++) {
            for (int col = 0; col < spec.matrixCols; col++) {
                intMatrix[row][col] = (row * 1_000) + col;
            }
        }

        double[][][] doubleCube = new double[8][16][16];
        for (int x = 0; x < doubleCube.length; x++) {
            for (int y = 0; y < doubleCube[x].length; y++) {
                for (int z = 0; z < doubleCube[x][y].length; z++) {
                    doubleCube[x][y][z] = x + (y / 100.0) + (z / 10_000.0);
                }
            }
        }

        return new PrimitiveArrays(bools, bytes, shorts, ints, longs, chars, floats, doubles, intMatrix, doubleCube);
    }

    private static CustomWithoutStatic[] createCustomArray(int size, int salt) {
        CustomWithoutStatic[] array = new CustomWithoutStatic[size];
        for (int i = 0; i < size; i++) {
            array[i] = new CustomWithoutStatic(
                    10_000_000L + i,
                    uniqueLabel("custom-array", i, salt),
                    new int[]{i, i + 1, i + 2}
            );
        }
        return array;
    }

    private static Object[] createMixedObjectArray(int size, CustomWithoutStatic[] customArray, int salt) {
        Object[] array = new Object[size];
        for (int i = 0; i < size; i++) {
            int mod = i % 6;
            if (mod == 0) {
                array[i] = customArray[i % customArray.length];
            } else if (mod == 1) {
                array[i] = Integer.valueOf(1_000_000 + (i * 11) + salt);
            } else if (mod == 2) {
                array[i] = Long.valueOf(10_000_000_000L + ((long) i * 19L) + salt);
            } else if (mod == 3) {
                array[i] = uniqueLabel("obj-array", i, salt);
            } else if (mod == 4) {
                array[i] = SampleEnum.values()[i % SampleEnum.values().length];
            } else {
                array[i] = new double[]{i * 0.1, i * 0.2, i * 0.3};
            }
        }
        return array;
    }

    private static List<Integer> createWrapperIntegerList(int size, int salt) {
        ArrayList<Integer> list = new ArrayList<>(size);
        for (int i = 0; i < size; i++) {
            list.add(Integer.valueOf(5_000_000 + (i * 23) + salt));
        }
        return list;
    }

    private static LinkedList<CustomWithoutStatic> createCustomLinkedList(int size, int salt) {
        LinkedList<CustomWithoutStatic> list = new LinkedList<>();
        for (int i = 0; i < size; i++) {
            list.add(new CustomWithoutStatic(
                    20_000_000L + i,
                    uniqueLabel("custom-list", i, salt),
                    new int[]{i, i * 2, i * 3, salt}
            ));
        }
        return list;
    }

    private static Deque<Long> createWrapperLongDeque(int size, int salt) {
        ArrayDeque<Long> deque = new ArrayDeque<>(size);
        for (int i = 0; i < size; i++) {
            deque.addLast(Long.valueOf(20_000_000_000L + ((long) i * 31L) + salt));
        }
        return deque;
    }

    private static Set<String> createStringSet(int size, int salt) {
        HashSet<String> set = new HashSet<>((size * 4 / 3) + 1);
        for (int i = 0; i < size; i++) {
            set.add(uniqueLabel("set-entry", i, salt));
        }
        return set;
    }

    private static Map<String, CustomWithoutStatic> createCustomMap(int size, int salt) {
        HashMap<String, CustomWithoutStatic> map = new HashMap<>((size * 4 / 3) + 1);
        for (int i = 0; i < size; i++) {
            String key = uniqueLabel("map-key", i, salt);
            map.put(key, new CustomWithoutStatic(
                    30_000_000L + i,
                    uniqueLabel("map-value", i, salt),
                    new int[]{salt, i, i + 7}
            ));
        }
        return map;
    }

    private static List<Long> createHugeWrapperLongList(int size, int salt) {
        ArrayList<Long> list = new ArrayList<>(size);
        for (int i = 0; i < size; i++) {
            list.add(Long.valueOf(90_000_000_000L + ((long) i * 37L) + salt));
        }
        return list;
    }

    private static CustomWithStatic createStaticCustomGraph(int nodeCount, int salt) {
        CustomWithStatic first = null;
        CustomWithStatic previous = null;
        for (int i = 0; i < nodeCount; i++) {
            CustomWithStatic node = new CustomWithStatic(
                    40_000_000L + i,
                    uniqueLabel("static-node", i, salt),
                    i * 0.125
            );
            if (first == null) {
                first = node;
            }
            if (previous != null) {
                previous.friend = node;
            }
            previous = node;
        }
        if (previous != null && first != null) {
            previous.friend = first;
        }
        return first;
    }

    private static CustomWithoutStatic createPlainCustomChain(int nodeCount, int salt) {
        CustomWithoutStatic first = null;
        CustomWithoutStatic previous = null;
        for (int i = 0; i < nodeCount; i++) {
            CustomWithoutStatic node = new CustomWithoutStatic(
                    50_000_000L + i,
                    uniqueLabel("plain-chain", i, salt),
                    new int[]{i, i + salt}
            );
            if (first == null) {
                first = node;
            }
            if (previous != null) {
                previous.next = node;
            }
            previous = node;
        }
        return first;
    }

    private static DirectCycle createDirectCycle(int salt) {
        DirectCycle node = new DirectCycle(uniqueLabel("direct-cycle", 0, salt));
        node.self = node;
        return node;
    }

    private static IndirectCycle2NodeA createIndirectCycle2(int salt) {
        IndirectCycle2NodeA nodeA = new IndirectCycle2NodeA(uniqueLabel("cycle2-A", 0, salt));
        IndirectCycle2NodeB nodeB = new IndirectCycle2NodeB(uniqueLabel("cycle2-B", 1, salt));
        nodeA.toB = nodeB;
        nodeB.toA = nodeA;
        return nodeA;
    }

    private static IndirectCycle3NodeA createIndirectCycle3(int salt) {
        IndirectCycle3NodeA nodeA = new IndirectCycle3NodeA(uniqueLabel("cycle3-A", 0, salt));
        IndirectCycle3NodeB nodeB = new IndirectCycle3NodeB(uniqueLabel("cycle3-B", 1, salt));
        IndirectCycle3NodeC nodeC = new IndirectCycle3NodeC(uniqueLabel("cycle3-C", 2, salt));
        nodeA.toB = nodeB;
        nodeB.toC = nodeC;
        nodeC.toA = nodeA;
        return nodeA;
    }

    private static String uniqueLabel(String prefix, int index, int salt) {
        return new StringBuilder(prefix.length() + 24)
                .append(prefix)
                .append('-')
                .append(index)
                .append('-')
                .append(salt)
                .toString();
    }

    private static void validateNoUnexpectedAliasing(Universe universe) {
        int customArrayDistinct = countDistinctIdentity(universe.customArray);
        if (customArrayDistinct != universe.customArray.length) {
            throw new IllegalStateException("custom array contains unexpected shared references");
        }

        int wrapperDistinct = countDistinctIdentity(universe.wrapperInts);
        if (wrapperDistinct != universe.wrapperInts.size()) {
            throw new IllegalStateException("wrapper list contains unexpected shared references");
        }

        int hugeDistinct = countDistinctIdentity(universe.hugeWrapperLongs);
        if (hugeDistinct != universe.hugeWrapperLongs.size()) {
            throw new IllegalStateException("huge list contains unexpected shared references");
        }
    }

    private static int countDistinctIdentity(Object[] values) {
        IdentityHashMap<Object, Boolean> seen = new IdentityHashMap<>();
        for (Object value : values) {
            seen.put(value, Boolean.TRUE);
        }
        return seen.size();
    }

    private static int countDistinctIdentity(List<?> values) {
        IdentityHashMap<Object, Boolean> seen = new IdentityHashMap<>();
        for (Object value : values) {
            seen.put(value, Boolean.TRUE);
        }
        return seen.size();
    }

    private static long computeChecksum(Universe universe) {
        long checksum = 17L;
        checksum = (checksum * 31) + universe.primitives.intValue;
        checksum = (checksum * 31) + universe.primitives.longValue;
        checksum = (checksum * 31) + universe.primitiveArrays.ints[13];
        checksum = (checksum * 31) + universe.wrapperInts.get(universe.wrapperInts.size() / 2);
        checksum = (checksum * 31) + universe.customLinkedList.get(universe.customLinkedList.size() / 2).id;
        if (!universe.hugeWrapperLongs.isEmpty()) {
            checksum = (checksum * 31) + universe.hugeWrapperLongs.get(universe.hugeWrapperLongs.size() / 2);
        }
        checksum = (checksum * 31) + universe.directCycle.name.length();
        checksum = (checksum * 31) + universe.indirectCycle2.label.length();
        checksum = (checksum * 31) + universe.indirectCycle3.label.length();
        return checksum;
    }

    private enum SampleEnum {
        ALPHA,
        BETA,
        GAMMA,
        DELTA
    }

    private static final class PrimitiveBox {
        private final boolean boolValue;
        private final byte byteValue;
        private final short shortValue;
        private final int intValue;
        private final long longValue;
        private final char charValue;
        private final float floatValue;
        private final double doubleValue;

        private PrimitiveBox(
                boolean boolValue,
                byte byteValue,
                short shortValue,
                int intValue,
                long longValue,
                char charValue,
                float floatValue,
                double doubleValue
        ) {
            this.boolValue = boolValue;
            this.byteValue = byteValue;
            this.shortValue = shortValue;
            this.intValue = intValue;
            this.longValue = longValue;
            this.charValue = charValue;
            this.floatValue = floatValue;
            this.doubleValue = doubleValue;
        }
    }

    private static final class PrimitiveArrays {
        private final boolean[] bools;
        private final byte[] bytes;
        private final short[] shorts;
        private final int[] ints;
        private final long[] longs;
        private final char[] chars;
        private final float[] floats;
        private final double[] doubles;
        private final int[][] intMatrix;
        private final double[][][] doubleCube;

        private PrimitiveArrays(
                boolean[] bools,
                byte[] bytes,
                short[] shorts,
                int[] ints,
                long[] longs,
                char[] chars,
                float[] floats,
                double[] doubles,
                int[][] intMatrix,
                double[][][] doubleCube
        ) {
            this.bools = bools;
            this.bytes = bytes;
            this.shorts = shorts;
            this.ints = ints;
            this.longs = longs;
            this.chars = chars;
            this.floats = floats;
            this.doubles = doubles;
            this.intMatrix = intMatrix;
            this.doubleCube = doubleCube;
        }
    }

    private static final class CustomWithoutStatic {
        private final long id;
        private final String label;
        private final int[] payload;
        private CustomWithoutStatic next;

        private CustomWithoutStatic(long id, String label, int[] payload) {
            this.id = id;
            this.label = label;
            this.payload = payload;
        }
    }

    private static final class CustomWithStatic {
        private static final Map<Long, String> STATIC_INDEX = new LinkedHashMap<>();
        private static final byte[] STATIC_PAYLOAD = new byte[8_192];
        private static long createdCount;

        static {
            for (int i = 0; i < STATIC_PAYLOAD.length; i++) {
                STATIC_PAYLOAD[i] = (byte) (i & 0xFF);
            }
        }

        private final long id;
        private final String name;
        private final double weight;
        private CustomWithStatic friend;

        private CustomWithStatic(long id, String name, double weight) {
            this.id = id;
            this.name = name;
            this.weight = weight;
            createdCount++;
            STATIC_INDEX.put(id, name);
        }
    }

    private static final class DirectCycle {
        private final String name;
        private DirectCycle self;

        private DirectCycle(String name) {
            this.name = name;
        }
    }

    private static final class IndirectCycle2NodeA {
        private final String label;
        private IndirectCycle2NodeB toB;

        private IndirectCycle2NodeA(String label) {
            this.label = label;
        }
    }

    private static final class IndirectCycle2NodeB {
        private final String label;
        private IndirectCycle2NodeA toA;

        private IndirectCycle2NodeB(String label) {
            this.label = label;
        }
    }

    private static final class IndirectCycle3NodeA {
        private final String label;
        private IndirectCycle3NodeB toB;

        private IndirectCycle3NodeA(String label) {
            this.label = label;
        }
    }

    private static final class IndirectCycle3NodeB {
        private final String label;
        private IndirectCycle3NodeC toC;

        private IndirectCycle3NodeB(String label) {
            this.label = label;
        }
    }

    private static final class IndirectCycle3NodeC {
        private final String label;
        private IndirectCycle3NodeA toA;

        private IndirectCycle3NodeC(String label) {
            this.label = label;
        }
    }

    private static final class Universe {
        private final ProfileSpec profileSpec;
        private final PrimitiveBox primitives;
        private final PrimitiveArrays primitiveArrays;
        private final Object[] mixedObjectArray;
        private final CustomWithoutStatic[] customArray;
        private final List<Integer> wrapperInts;
        private final LinkedList<CustomWithoutStatic> customLinkedList;
        private final Deque<Long> wrapperLongDeque;
        private final Set<String> wrapperStringSet;
        private final Map<String, CustomWithoutStatic> customMap;
        private final List<Long> hugeWrapperLongs;
        private final CustomWithStatic staticRoot;
        private final CustomWithoutStatic plainRoot;
        private final SampleEnum enumValue;
        private final DirectCycle directCycle;
        private final IndirectCycle2NodeA indirectCycle2;
        private final IndirectCycle3NodeA indirectCycle3;

        private Universe(
                ProfileSpec profileSpec,
                PrimitiveBox primitives,
                PrimitiveArrays primitiveArrays,
                Object[] mixedObjectArray,
                CustomWithoutStatic[] customArray,
                List<Integer> wrapperInts,
                LinkedList<CustomWithoutStatic> customLinkedList,
                Deque<Long> wrapperLongDeque,
                Set<String> wrapperStringSet,
                Map<String, CustomWithoutStatic> customMap,
                List<Long> hugeWrapperLongs,
                CustomWithStatic staticRoot,
                CustomWithoutStatic plainRoot,
                SampleEnum enumValue,
                DirectCycle directCycle,
                IndirectCycle2NodeA indirectCycle2,
                IndirectCycle3NodeA indirectCycle3
        ) {
            this.profileSpec = profileSpec;
            this.primitives = primitives;
            this.primitiveArrays = primitiveArrays;
            this.mixedObjectArray = mixedObjectArray;
            this.customArray = customArray;
            this.wrapperInts = wrapperInts;
            this.customLinkedList = customLinkedList;
            this.wrapperLongDeque = wrapperLongDeque;
            this.wrapperStringSet = wrapperStringSet;
            this.customMap = customMap;
            this.hugeWrapperLongs = hugeWrapperLongs;
            this.staticRoot = staticRoot;
            this.plainRoot = plainRoot;
            this.enumValue = enumValue;
            this.directCycle = directCycle;
            this.indirectCycle2 = indirectCycle2;
            this.indirectCycle3 = indirectCycle3;
        }
    }

    private static final class FrameRootWorker {
        private final CountDownLatch ready = new CountDownLatch(1);
        private final Thread thread;
        private volatile boolean running = true;

        private FrameRootWorker(int localObjectCount) {
            this.thread = new Thread(() -> run(localObjectCount), "scenario-01-frame-root-worker");
        }

        void startAndAwaitReady() throws InterruptedException {
            this.thread.start();
            this.ready.await();
        }

        void stopAndJoin() throws InterruptedException {
            running = false;
            thread.join();
        }

        private void run(int localObjectCount) {
            CustomWithoutStatic localCustom = new CustomWithoutStatic(
                    88_000_000L,
                    "frame-local-root",
                    new int[]{7, 14, 21}
            );
            int[] localInts = new int[Math.max(256, localObjectCount)];
            Object[] localObjects = new Object[Math.max(64, localObjectCount)];
            int[][] localMatrix = new int[32][32];

            for (int i = 0; i < localObjects.length; i++) {
                localObjects[i] = new CustomWithoutStatic(
                        89_000_000L + i,
                        "frame-local-" + i,
                        new int[]{i, i + 1}
                );
            }

            ready.countDown();

            long iteration = 0L;
            while (running) {
                int objIndex = (int) (iteration % localObjects.length);
                int intIndex = (int) (iteration % localInts.length);
                localInts[intIndex] = (int) iteration;
                localMatrix[objIndex % localMatrix.length][(objIndex * 3) % localMatrix[0].length] = (int) (iteration & 0x7FFF);
                if ((iteration & 31L) == 0L) {
                    FixtureRuntime.touch(localObjects[objIndex]);
                }
                if ((iteration & 127L) == 0L) {
                    FixtureRuntime.touch(localCustom);
                }
                iteration++;
                LockSupport.parkNanos(1_000_000L);
            }

            FixtureRuntime.touch(localObjects[0]);
        }
    }
}
