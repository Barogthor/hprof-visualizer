import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public final class Scenario10StringExtremes implements HeapScenario {
    @Override
    public String id() {
        return "10";
    }

    @Override
    public String name() {
        return "string-extremes";
    }

    @Override
    public ScenarioHandle setup(ProfileSpec spec) {
        StringRoot root = new StringRoot();
        root.longStrings = new ArrayList<>();
        root.utfStrings = new ArrayList<>();
        root.similarPrefixStrings = new ArrayList<>();
        root.stringMap = new LinkedHashMap<>();
        root.charArrays = new ArrayList<>();

        int longCount = Math.max(64, spec.objectArraySize / 8);
        int longSize = Math.max(4_096, spec.matrixRows * spec.matrixCols / 2);
        for (int i = 0; i < longCount; i++) {
            root.longStrings.add(repeatedPattern("long-" + i + '-' + spec.seed + '-', longSize));
        }

        String[] utfSeeds = {
                "Français-été",
                "Español-año",
                "Deutsch-äöüß",
                "Polski-zażółć",
                "Greek-αλφα",
                "Cyrillic-пример",
                "Arabic-مرحبا",
                "Japanese-こんにちは",
                "Korean-안녕하세요",
                "Emoji-🙂🚀✨"
        };

        int utfCount = Math.max(2_048, spec.wrapperCollectionSize / 4);
        for (int i = 0; i < utfCount; i++) {
            String seed = utfSeeds[i % utfSeeds.length];
            String value = seed + "-idx-" + i + "-profile-" + spec.name;
            root.utfStrings.add(new String(value));
        }

        int similarCount = Math.max(10_000, spec.wrapperCollectionSize);
        for (int i = 0; i < similarCount; i++) {
            String value = "user-session-path-" + spec.seed + "-token-" + i + "-region-eu-west";
            root.similarPrefixStrings.add(new String(value));
        }

        for (int i = 0; i < 512; i++) {
            String key = "k-" + i;
            String value = (i % 3 == 0)
                    ? root.similarPrefixStrings.get(i)
                    : root.utfStrings.get(i % root.utfStrings.size());
            root.stringMap.put(key, value);
        }

        int charArraysCount = Math.max(64, spec.objectArraySize / 16);
        int charsLen = Math.max(1024, spec.matrixCols * 4);
        for (int i = 0; i < charArraysCount; i++) {
            char[] chars = new char[charsLen];
            for (int j = 0; j < chars.length; j++) {
                chars[j] = (char) ('A' + ((i + j) % 26));
            }
            root.charArrays.add(chars);
        }

        Map<String, String> metrics = new LinkedHashMap<>();
        metrics.put("scenario", "very long strings + utf variants + similar prefixes");
        metrics.put("longStrings", Integer.toString(root.longStrings.size()));
        metrics.put("utfStrings", Integer.toString(root.utfStrings.size()));
        metrics.put("similarPrefixStrings", Integer.toString(root.similarPrefixStrings.size()));
        metrics.put("charArrays", Integer.toString(root.charArrays.size()));

        List<Object> roots = new ArrayList<>();
        roots.add(root);
        return new ScenarioHandle(roots, metrics, null);
    }

    private static String repeatedPattern(String pattern, int targetLen) {
        StringBuilder builder = new StringBuilder(targetLen);
        while (builder.length() < targetLen) {
            builder.append(pattern);
        }
        if (builder.length() > targetLen) {
            builder.setLength(targetLen);
        }
        return builder.toString();
    }

    private static final class StringRoot {
        private List<String> longStrings;
        private List<String> utfStrings;
        private List<String> similarPrefixStrings;
        private Map<String, String> stringMap;
        private List<char[]> charArrays;
    }
}
