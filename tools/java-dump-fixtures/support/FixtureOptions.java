import java.nio.file.Path;
import java.nio.file.Paths;

public final class FixtureOptions {
    public final String profileName;
    public final Path outputPath;
    public final DumpMode dumpMode;
    public final long holdSeconds;
    public final long truncateBytes;
    public final String scenarioId;
    public final boolean help;

    private FixtureOptions(
            String profileName,
            Path outputPath,
            DumpMode dumpMode,
            long holdSeconds,
            long truncateBytes,
            String scenarioId,
            boolean help
    ) {
        this.profileName = profileName;
        this.outputPath = outputPath;
        this.dumpMode = dumpMode;
        this.holdSeconds = holdSeconds;
        this.truncateBytes = truncateBytes;
        this.scenarioId = scenarioId;
        this.help = help;
    }

    public static FixtureOptions parse(String[] args) {
        String profileName = "medium";
        Path outputPath = null;
        DumpMode dumpMode = DumpMode.BOTH;
        long holdSeconds = 120L;
        long truncateBytes = 0L;
        String scenarioId = "01";
        boolean help = false;

        for (int i = 0; i < args.length; i++) {
            String arg = args[i];
            if ("--profile".equals(arg)) {
                profileName = readValue(args, ++i, "--profile");
            } else if ("--output".equals(arg)) {
                outputPath = Paths.get(readValue(args, ++i, "--output"));
            } else if ("--dump-mode".equals(arg)) {
                dumpMode = DumpMode.fromText(readValue(args, ++i, "--dump-mode"));
            } else if ("--hold-seconds".equals(arg)) {
                holdSeconds = Long.parseLong(readValue(args, ++i, "--hold-seconds"));
            } else if ("--truncate-bytes".equals(arg)) {
                truncateBytes = Long.parseLong(readValue(args, ++i, "--truncate-bytes"));
            } else if ("--scenario".equals(arg)) {
                scenarioId = normalizeScenarioId(readValue(args, ++i, "--scenario"));
            } else if ("--help".equals(arg) || "-h".equals(arg)) {
                help = true;
            } else {
                throw new IllegalArgumentException("Unknown argument: " + arg);
            }
        }

        if (holdSeconds < 1L) {
            throw new IllegalArgumentException("--hold-seconds must be >= 1");
        }
        if (truncateBytes < 0L) {
            throw new IllegalArgumentException("--truncate-bytes must be >= 0");
        }

        return new FixtureOptions(profileName, outputPath, dumpMode, holdSeconds, truncateBytes, scenarioId, help);
    }

    private static String readValue(String[] args, int index, String optionName) {
        if (index >= args.length) {
            throw new IllegalArgumentException("Missing value for " + optionName);
        }
        return args[index];
    }

    private static String normalizeScenarioId(String input) {
        if ("all".equalsIgnoreCase(input)) {
            return "all";
        }
        if (input.length() == 1 && Character.isDigit(input.charAt(0))) {
            return "0" + input;
        }
        return input;
    }
}
