import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public final class Scenario05HugeObjects implements HeapScenario {
    @Override
    public String id() {
        return "05";
    }

    @Override
    public String name() {
        return "huge-objects";
    }

    @Override
    public ScenarioHandle setup(ProfileSpec spec) {
        HeavyRoot root = new HeavyRoot();
        root.byteBlocks = new ArrayList<>(spec.heavyBlockCount);
        root.charBlocks = new ArrayList<>(Math.max(2, spec.heavyBlockCount / 2));
        root.longList = new ArrayList<>(Math.max(8_192, spec.hugeCollectionSize));
        root.mixedHugeArray = new Object[Math.max(2_048, spec.objectArraySize)];

        int bytesPerBlock = spec.heavyBlockMiB * 1024 * 1024;
        for (int i = 0; i < spec.heavyBlockCount; i++) {
            byte[] block = new byte[bytesPerBlock];
            for (int j = 0; j < block.length; j += 4096) {
                block[j] = (byte) ((i + j) & 0xFF);
            }
            root.byteBlocks.add(block);
        }

        for (int i = 0; i < Math.max(2, spec.heavyBlockCount / 2); i++) {
            char[] chars = new char[Math.max(262_144, bytesPerBlock / 2)];
            for (int j = 0; j < chars.length; j += 1024) {
                chars[j] = (char) ('A' + ((i + j) % 26));
            }
            root.charBlocks.add(chars);
        }

        int longListSize = spec.hugeCollectionSize > 0 ? spec.hugeCollectionSize : spec.wrapperCollectionSize;
        for (int i = 0; i < longListSize; i++) {
            root.longList.add(Long.valueOf(100_000_000_000L + ((long) i * 13L)));
        }

        for (int i = 0; i < root.mixedHugeArray.length; i++) {
            int mod = i % 5;
            if (mod == 0) {
                root.mixedHugeArray[i] = root.byteBlocks.get(i % root.byteBlocks.size());
            } else if (mod == 1) {
                root.mixedHugeArray[i] = root.charBlocks.get(i % root.charBlocks.size());
            } else if (mod == 2) {
                root.mixedHugeArray[i] = root.longList.get(i % root.longList.size());
            } else if (mod == 3) {
                root.mixedHugeArray[i] = "huge-" + i + "-" + spec.seed;
            } else {
                root.mixedHugeArray[i] = new int[]{i, i + 1, i + 2, i + 3};
            }
        }

        long totalBytes = ((long) root.byteBlocks.size() * bytesPerBlock)
                + ((long) root.charBlocks.size() * root.charBlocks.get(0).length * 2L);

        Map<String, String> metrics = new LinkedHashMap<>();
        metrics.put("scenario", "huge arrays + large wrappers");
        metrics.put("byteBlocks", Integer.toString(root.byteBlocks.size()));
        metrics.put("charBlocks", Integer.toString(root.charBlocks.size()));
        metrics.put("longListSize", Integer.toString(root.longList.size()));
        metrics.put("approxHeavyBytes", Long.toString(totalBytes));

        List<Object> roots = new ArrayList<>();
        roots.add(root);
        return new ScenarioHandle(roots, metrics, null);
    }

    private static final class HeavyRoot {
        private List<byte[]> byteBlocks;
        private List<char[]> charBlocks;
        private List<Long> longList;
        private Object[] mixedHugeArray;
    }
}
