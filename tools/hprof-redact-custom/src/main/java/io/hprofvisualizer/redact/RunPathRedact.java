package io.hprofvisualizer.redact;

import me.bechberger.hprof.HprofRedact;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;

public final class RunPathRedact {
    private RunPathRedact() {
    }

    public static void main(String[] args) throws Exception {
        if (args.length == 0 || isHelp(args[0])) {
            printHelp();
            return;
        }
        if (args.length != 2) {
            System.err.println("Invalid arguments.");
            printHelp();
            System.exit(1);
            return;
        }

        Path input = Paths.get(args[0]);
        Path requestedOutput = Paths.get(args[1]);
        Path actualOutput = ensureWritableOutputPath(requestedOutput);

        HprofRedact.process(input, actualOutput, new PathOnlyTransformer());

        System.out.println("input=" + input.toAbsolutePath().normalize());
        if (!actualOutput.equals(requestedOutput)) {
            System.out.println("requestedOutput=" + requestedOutput.toAbsolutePath().normalize());
        }
        System.out.println("output=" + actualOutput.toAbsolutePath().normalize());
        System.out.println("transformer=path-only");
    }

    private static Path ensureWritableOutputPath(Path requestedOutput) throws IOException {
        if (!Files.exists(requestedOutput)) {
            return requestedOutput;
        }

        try {
            Files.delete(requestedOutput);
            return requestedOutput;
        } catch (IOException deleteError) {
            Path fallback = nextAvailablePath(requestedOutput);
            System.out.println("note=output_exists_or_locked_using_fallback");
            return fallback;
        }
    }

    private static Path nextAvailablePath(Path base) {
        String fileName = base.getFileName().toString();
        int dot = fileName.lastIndexOf('.');
        String stem = dot > 0 ? fileName.substring(0, dot) : fileName;
        String ext = dot > 0 ? fileName.substring(dot) : ".hprof";

        for (int i = 1; i < 1000; i++) {
            Path candidate = base.resolveSibling(stem + "-" + i + ext);
            if (!Files.exists(candidate)) {
                return candidate;
            }
        }

        return base.resolveSibling(stem + "-fallback" + ext);
    }

    private static boolean isHelp(String value) {
        return "-h".equals(value) || "--help".equals(value) || "help".equalsIgnoreCase(value);
    }

    private static void printHelp() {
        System.out.println("Usage:");
        System.out.println("  java -jar target/hprof-path-redact.jar <input.hprof> <output.hprof>");
        System.out.println();
        System.out.println("Behavior:");
        System.out.println("  - Redacts sensitive path-like UTF-8 strings (Windows, UNC, Unix)");
        System.out.println("  - Redacts known env var values (PATH, JAVA_HOME, USERPROFILE, etc.)");
        System.out.println("  - Preserves Java symbol-like strings (class names/signatures)");
        System.out.println("  - Leaves primitive values unchanged");
    }
}
