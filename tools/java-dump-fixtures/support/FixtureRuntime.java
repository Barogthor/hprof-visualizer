import java.util.ArrayList;
import java.util.Collection;
import java.util.List;

public final class FixtureRuntime {
    private static final List<Object> ROOTS = new ArrayList<>();
    private static volatile Object blackHole;

    private FixtureRuntime() {
    }

    public static void pin(Object root) {
        ROOTS.add(root);
        blackHole = root;
    }

    public static void pinAll(Collection<?> roots) {
        for (Object root : roots) {
            pin(root);
        }
    }

    public static void touch(Object value) {
        blackHole = value;
    }

    public static void clear() {
        ROOTS.clear();
        blackHole = null;
    }
}
