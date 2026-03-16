import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;

public final class HeapDumpFixture {
    private static final Map<String, HeapScenario> SCENARIOS = buildScenarios();

    private HeapDumpFixture() {
    }

    public static void main(String[] args) throws Exception {
        FixtureOptions options = FixtureOptions.parse(args);
        if (options.help) {
            printHelp();
            return;
        }

        ProfileSpec spec = ProfileSpec.fromName(options.profileName);
        List<HeapScenario> selectedScenarios = selectScenarios(options.scenarioId);

        for (HeapScenario scenario : selectedScenarios) {
            validateProfileScenario(spec, scenario);
            Path baseOutput = resolveOutputPath(options.outputPath, spec, scenario.id(), selectedScenarios.size() > 1);
            runScenario(options, spec, scenario, baseOutput);
        }
    }

    private static Map<String, HeapScenario> buildScenarios() {
        Map<String, HeapScenario> scenarios = new LinkedHashMap<>();
        register(scenarios, new Scenario01StackFrameTypes());
        register(scenarios, new Scenario02CollectionsTopology());
        register(scenarios, new Scenario03LeakPatterns());
        register(scenarios, new Scenario04ReferenceTypes());
        register(scenarios, new Scenario05HugeObjects());
        register(scenarios, new Scenario06Deadlock());
        register(scenarios, new Scenario07ThreadLocalPoolLeak());
        register(scenarios, new Scenario08ClassLoaderRetention());
        register(scenarios, new Scenario09ConcurrentMapHotBuckets());
        register(scenarios, new Scenario10StringExtremes());
        register(scenarios, new Scenario11StressLoading());
        return scenarios;
    }

    private static void register(Map<String, HeapScenario> scenarios, HeapScenario scenario) {
        scenarios.put(scenario.id(), scenario);
    }

    private static List<HeapScenario> selectScenarios(String scenarioId) {
        if ("all".equalsIgnoreCase(scenarioId)) {
            return new ArrayList<>(SCENARIOS.values());
        }

        HeapScenario scenario = SCENARIOS.get(normalizeScenarioId(scenarioId));
        if (scenario == null) {
            throw new IllegalArgumentException("Unknown scenario: " + scenarioId);
        }
        List<HeapScenario> one = new ArrayList<>();
        one.add(scenario);
        return one;
    }

    private static String normalizeScenarioId(String scenarioId) {
        if (scenarioId == null) {
            return "01";
        }
        if (scenarioId.length() == 1 && Character.isDigit(scenarioId.charAt(0))) {
            return "0" + scenarioId;
        }
        return scenarioId;
    }

    private static void validateProfileScenario(
            ProfileSpec spec, HeapScenario scenario) {
    }

    private static Path resolveOutputPath(Path configuredOutput, ProfileSpec spec, String scenarioId, boolean appendScenarioSuffix) {
        if (configuredOutput == null) {
            return Paths.get("assets", "generated", "fixture-s" + scenarioId + "-" + spec.name + ".hprof");
        }
        if (!appendScenarioSuffix) {
            return configuredOutput;
        }
        return DumpSupport.withSuffix(configuredOutput, "-s" + scenarioId);
    }

    private static void runScenario(FixtureOptions options, ProfileSpec spec, HeapScenario scenario, Path baseOutput) throws Exception {
        FixtureRuntime.clear();
        try (ScenarioHandle handle = scenario.setup(spec)) {
            FixtureRuntime.pinAll(handle.roots());

            long pid = ProcessHandle.current().pid();
            System.out.println("scenarioId=" + scenario.id());
            System.out.println("scenarioName=" + scenario.name());
            System.out.println("profile=" + spec.name);
            System.out.println("pid=" + pid);
            System.out.println("dumpMode=" + options.dumpMode.name().toLowerCase(Locale.ROOT));
            System.out.println("truncateBytes=" + options.truncateBytes);
            for (Map.Entry<String, String> entry : handle.metrics().entrySet()) {
                System.out.println(entry.getKey() + "=" + entry.getValue());
            }

            List<Path> dumpCandidates = new ArrayList<>();

            if (options.dumpMode == DumpMode.AUTO || options.dumpMode == DumpMode.BOTH) {
                Path autoPath = options.dumpMode == DumpMode.BOTH
                        ? DumpSupport.withSuffix(baseOutput, "-auto")
                        : baseOutput;
                dumpCandidates.add(autoPath);
                DumpSupport.dumpHeap(autoPath, true);
            }

            if (options.dumpMode == DumpMode.MANUAL || options.dumpMode == DumpMode.BOTH) {
                Path jcmdPath = options.dumpMode == DumpMode.BOTH
                        ? DumpSupport.withSuffix(baseOutput, "-jcmd")
                        : baseOutput;
                Path jmapPath = DumpSupport.withSuffix(jcmdPath, "-jmap");

                dumpCandidates.add(jcmdPath);
                dumpCandidates.add(jmapPath);

                DumpSupport.printManualInstructions(pid, jcmdPath, jmapPath);
                DumpSupport.holdForManualDump(options.holdSeconds);
            }

            if (options.truncateBytes > 0L) {
                int truncatedCount = DumpSupport.createTruncatedCopies(dumpCandidates, options.truncateBytes);
                System.out.println("truncatedDumpCount=" + truncatedCount);
            }
        } finally {
            FixtureRuntime.clear();
        }
    }

    private static void printHelp() {
        System.out.println("HeapDumpFixture options:");
        System.out.println("  --scenario <01|02|...|10|11|all>");
        System.out.println("  --profile <tiny|medium|large|xlarge|ultra>");
        System.out.println("  --output <path/to/file.hprof>");
        System.out.println("  --dump-mode <auto|manual|both>");
        System.out.println("  --hold-seconds <seconds>");
        System.out.println("  --truncate-bytes <bytes>");
        System.out.println("  --help");
        System.out.println("Scenarios:");
        for (HeapScenario scenario : SCENARIOS.values()) {
            System.out.println("  " + scenario.id() + " -> " + scenario.name());
        }
    }
}
