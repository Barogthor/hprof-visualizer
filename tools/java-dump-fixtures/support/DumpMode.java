import java.util.Locale;

public enum DumpMode {
    AUTO,
    MANUAL,
    BOTH;

    public static DumpMode fromText(String value) {
        String lowered = value.toLowerCase(Locale.ROOT);
        if ("auto".equals(lowered)) {
            return AUTO;
        }
        if ("manual".equals(lowered)) {
            return MANUAL;
        }
        if ("both".equals(lowered)) {
            return BOTH;
        }
        throw new IllegalArgumentException("Unsupported dump mode: " + value);
    }
}
