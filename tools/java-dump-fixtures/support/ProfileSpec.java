import java.util.Locale;

public final class ProfileSpec {
    public final String name;
    public final int seed;
    public final int wrapperCollectionSize;
    public final int customCollectionSize;
    public final int objectArraySize;
    public final int customArraySize;
    public final int matrixRows;
    public final int matrixCols;
    public final int customGraphNodeCount;
    public final int hugeCollectionSize;
    public final int frameRootObjectCount;
    public final int heavyBlockMiB;
    public final int heavyBlockCount;

    private ProfileSpec(
            String name,
            int seed,
            int wrapperCollectionSize,
            int customCollectionSize,
            int objectArraySize,
            int customArraySize,
            int matrixRows,
            int matrixCols,
            int customGraphNodeCount,
            int hugeCollectionSize,
            int frameRootObjectCount,
            int heavyBlockMiB,
            int heavyBlockCount
    ) {
        this.name = name;
        this.seed = seed;
        this.wrapperCollectionSize = wrapperCollectionSize;
        this.customCollectionSize = customCollectionSize;
        this.objectArraySize = objectArraySize;
        this.customArraySize = customArraySize;
        this.matrixRows = matrixRows;
        this.matrixCols = matrixCols;
        this.customGraphNodeCount = customGraphNodeCount;
        this.hugeCollectionSize = hugeCollectionSize;
        this.frameRootObjectCount = frameRootObjectCount;
        this.heavyBlockMiB = heavyBlockMiB;
        this.heavyBlockCount = heavyBlockCount;
    }

    public static ProfileSpec fromName(String name) {
        String lowered = name.toLowerCase(Locale.ROOT);
        if ("tiny".equals(lowered)) {
            return new ProfileSpec("tiny", 101, 10_240, 10_240, 512, 1_024, 48, 48, 128, 0, 256, 4, 4);
        }
        if ("medium".equals(lowered)) {
            return new ProfileSpec("medium", 202, 14_336, 14_336, 1_024, 2_048, 64, 64, 256, 0, 512, 8, 6);
        }
        if ("large".equals(lowered)) {
            return new ProfileSpec("large", 303, 20_480, 20_480, 2_048, 4_096, 96, 96, 512, 0, 1_024, 12, 8);
        }
        if ("xlarge".equals(lowered)) {
            return new ProfileSpec("xlarge", 404, 30_720, 30_720, 4_096, 8_192, 128, 128, 768, 500_000, 2_048, 16, 10);
        }
        if ("ultra".equals(lowered)) {
            return new ProfileSpec("ultra", 505, 65_536, 65_536, 16_384, 32_768, 256, 256, 2_048, 1_000_000, 8_192, 20, 12);
        }
        throw new IllegalArgumentException("Unsupported profile: " + name);
    }
}
