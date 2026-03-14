public interface HeapScenario {
    String id();

    String name();

    ScenarioHandle setup(ProfileSpec spec) throws Exception;
}
