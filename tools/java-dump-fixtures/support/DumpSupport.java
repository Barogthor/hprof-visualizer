import com.sun.management.HotSpotDiagnosticMXBean;

import java.io.InputStream;
import java.io.OutputStream;
import java.lang.management.ManagementFactory;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import java.time.Duration;
import java.util.List;
import java.util.concurrent.TimeUnit;

public final class DumpSupport {
    private static final String HOTSPOT_BEAN_NAME = "com.sun.management:type=HotSpotDiagnostic";

    private DumpSupport() {
    }

    public static void dumpHeap(Path output, boolean live) throws Exception {
        Path absolute = output.toAbsolutePath().normalize();
        Path parent = absolute.getParent();
        if (parent != null) {
            Files.createDirectories(parent);
        }
        Files.deleteIfExists(absolute);

        HotSpotDiagnosticMXBean bean = ManagementFactory.newPlatformMXBeanProxy(
                ManagementFactory.getPlatformMBeanServer(),
                HOTSPOT_BEAN_NAME,
                HotSpotDiagnosticMXBean.class
        );

        long startNanos = System.nanoTime();
        bean.dumpHeap(absolute.toString(), live);
        Duration elapsed = Duration.ofNanos(System.nanoTime() - startNanos);
        System.out.println("autoDumpPath=" + absolute);
        System.out.println("autoDumpDurationMs=" + elapsed.toMillis());
    }

    public static void printManualInstructions(long pid, Path jcmdPath, Path jmapPath) {
        Path absJcmd = jcmdPath.toAbsolutePath().normalize();
        Path absJmap = jmapPath.toAbsolutePath().normalize();

        System.out.println("manualDumpInstructions:");
        System.out.println("  jcmd " + pid + " GC.heap_dump " + absJcmd);
        System.out.println("  jmap -dump:live,format=b,file=" + absJmap + " " + pid);
    }

    public static void holdForManualDump(long holdSeconds) throws InterruptedException {
        long deadlineNanos = System.nanoTime() + TimeUnit.SECONDS.toNanos(holdSeconds);
        long previousRemaining = -1L;
        while (System.nanoTime() < deadlineNanos) {
            long remaining = TimeUnit.NANOSECONDS.toSeconds(deadlineNanos - System.nanoTime());
            if (remaining != previousRemaining && (remaining <= 10 || remaining % 10 == 0)) {
                System.out.println("manualWindowRemainingSeconds=" + remaining);
                previousRemaining = remaining;
            }
            Thread.sleep(200L);
        }
    }

    public static int createTruncatedCopies(List<Path> dumpCandidates, long truncateBytes) throws Exception {
        int created = 0;
        for (Path candidate : dumpCandidates) {
            Path absolute = candidate.toAbsolutePath().normalize();
            if (!Files.isRegularFile(absolute)) {
                System.out.println("truncateSkippedMissing=" + absolute);
                continue;
            }

            long originalSize = Files.size(absolute);
            if (originalSize <= 1L) {
                System.out.println("truncateSkippedTooSmall=" + absolute);
                continue;
            }

            long keptBytes = originalSize - truncateBytes;
            if (keptBytes < 1L) {
                keptBytes = 1L;
            }

            Path truncated = withSuffix(absolute, "-truncated");
            Files.deleteIfExists(truncated);
            copyFirstBytes(absolute, truncated, keptBytes);

            long truncatedSize = Files.size(truncated);
            System.out.println("truncatedDumpPath=" + truncated + " original=" + originalSize + " truncated=" + truncatedSize);
            created++;
        }
        return created;
    }

    public static Path withSuffix(Path path, String suffix) {
        String fileName = path.getFileName().toString();
        int dotIndex = fileName.lastIndexOf('.');
        String base = dotIndex > 0 ? fileName.substring(0, dotIndex) : fileName;
        String extension = dotIndex > 0 ? fileName.substring(dotIndex) : ".hprof";
        String newName = base + suffix + extension;
        return path.resolveSibling(newName);
    }

    private static void copyFirstBytes(Path input, Path output, long bytesToCopy) throws Exception {
        try (
                InputStream in = Files.newInputStream(input, StandardOpenOption.READ);
                OutputStream out = Files.newOutputStream(output, StandardOpenOption.CREATE_NEW, StandardOpenOption.WRITE)
        ) {
            byte[] buffer = new byte[8192];
            long remaining = bytesToCopy;
            while (remaining > 0L) {
                int maxRead = (int) Math.min(buffer.length, remaining);
                int read = in.read(buffer, 0, maxRead);
                if (read < 0) {
                    break;
                }
                out.write(buffer, 0, read);
                remaining -= read;
            }
            out.flush();
        }
    }
}
