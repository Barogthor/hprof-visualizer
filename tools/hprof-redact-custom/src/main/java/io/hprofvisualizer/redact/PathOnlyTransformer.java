package io.hprofvisualizer.redact;

import me.bechberger.hprof.transformer.HprofTransformer;

import java.nio.charset.Charset;
import java.nio.charset.StandardCharsets;
import java.util.Locale;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

public final class PathOnlyTransformer implements HprofTransformer {
    private static final Pattern JAVA_SYMBOL = Pattern.compile(
            "^(?:\\[*L[\\w/$]+;|[\\w$]+(?:/[\\w$]+)+|\\(.*\\)[A-Z\\[V]|<.*>)$"
    );

    private static final Pattern WINDOWS_PATH = Pattern.compile(
            "(?<!\\w)(?:[A-Za-z]:[\\\\/][\\w .()\\-\\\\/]{4,300})"
    );
    private static final Pattern WINDOWS_UNC_PATH = Pattern.compile(
            "(?<!\\w)(?:\\\\\\\\[\\w.$ -]+\\\\[\\w.$ -\\\\/]{2,300})"
    );
    private static final Pattern UNIX_PATH = Pattern.compile(
            "(?<!\\w)(?:/(?:home|Users|opt|usr|usr/local|var|tmp|etc|srv|mnt|media|root|private|Library)/[\\w .()\\-\\\\/]{3,300})"
    );

    private static final Pattern JVM_AGENT_PATH = Pattern.compile(
            "-(?:javaagent|agentpath|agentlib):([^\\s\"']+)"
    );
    private static final Pattern JVM_BOOTCLASSPATH = Pattern.compile(
            "-Xbootclasspath(?:/p)?:([^\\s\"']+)"
    );
    private static final Pattern JVM_D_PATH_VALUE = Pattern.compile(
            "-D[\\w.]{1,80}=([A-Za-z]:[\\\\/][^\\s\"']+|/(?:home|Users|opt|usr|usr/local|var|tmp|etc|srv|mnt|media|root|private|Library)/[^\\s\"']+)"
    );
    private static final Pattern JVM_D_USER_OS_VALUE = Pattern.compile(
            "-D(?:user|os)\\.[\\w.-]{1,80}=([^\\s\"']+)"
    );

    private static final Set<String> SENSITIVE_PREFIXES = Set.of(
            "PATH=",
            "JAVA_HOME=",
            "JRE_HOME=",
            "JDK_HOME=",
            "CLASSPATH=",
            "LD_LIBRARY_PATH=",
            "HOME=",
            "TMPDIR=",
            "TEMP=",
            "TMP=",
            "USERPROFILE=",
            "APPDATA=",
            "LOCALAPPDATA=",
            "PROGRAMFILES=",
            "PROGRAMFILES(X86)="
    );

    @Override
    public String transformUtf8(String value) {
        return transformUtf8String(value);
    }

    @Override
    public String transformUtf8String(String value) {
        if (value == null || value.isEmpty()) {
            return value;
        }

        return redactCandidate(value);
    }

    @Override
    public void transformCharArray(char[] value) {
        if (value == null || value.length == 0) {
            return;
        }

        String original = new String(value);
        String redacted = redactCandidate(original);
        if (!original.equals(redacted)) {
            redacted.getChars(0, value.length, value, 0);
        }
    }

    @Override
    public void transformByteArray(byte[] value) {
        if (value == null || value.length == 0) {
            return;
        }

        if (tryRedactWithCharset(value, StandardCharsets.ISO_8859_1, looksMostlyPrintable(value))) {
            return;
        }

        if (looksLikeUtf16(value)) {
            if (tryRedactWithCharset(value, StandardCharsets.UTF_16BE, true)) {
                return;
            }
            tryRedactWithCharset(value, StandardCharsets.UTF_16LE, true);
        }
    }

    private static String redactCandidate(String value) {
        if (JAVA_SYMBOL.matcher(value).matches()) {
            return value;
        }

        if (startsWithSensitivePrefix(value)) {
            return maskAfterEquals(value);
        }

        String redacted = value;
        redacted = redactMatchGroup(redacted, JVM_AGENT_PATH, 1);
        redacted = redactMatchGroup(redacted, JVM_BOOTCLASSPATH, 1);
        redacted = redactMatchGroup(redacted, JVM_D_PATH_VALUE, 1);
        redacted = redactMatchGroup(redacted, JVM_D_USER_OS_VALUE, 1);

        redacted = redactWholeMatches(redacted, WINDOWS_PATH);
        redacted = redactWholeMatches(redacted, WINDOWS_UNC_PATH);
        redacted = redactWholeMatches(redacted, UNIX_PATH);
        return redacted;
    }

    private static boolean tryRedactWithCharset(byte[] bytes, Charset charset, boolean shouldTry) {
        if (!shouldTry) {
            return false;
        }

        String original = new String(bytes, charset);
        String redacted = redactCandidate(original);
        if (original.equals(redacted)) {
            return false;
        }

        byte[] updated = redacted.getBytes(charset);
        if (updated.length != bytes.length) {
            return false;
        }

        System.arraycopy(updated, 0, bytes, 0, bytes.length);
        return true;
    }

    private static boolean looksMostlyPrintable(byte[] bytes) {
        int printable = 0;
        for (byte b : bytes) {
            int v = b & 0xFF;
            if (v == 9 || v == 10 || v == 13 || (v >= 32 && v <= 126)) {
                printable++;
            }
        }
        return printable >= (bytes.length * 3 / 4);
    }

    private static boolean looksLikeUtf16(byte[] bytes) {
        if (bytes.length < 4 || (bytes.length & 1) != 0) {
            return false;
        }

        int zerosEven = 0;
        int zerosOdd = 0;
        int pairs = bytes.length / 2;

        for (int i = 0; i < bytes.length; i += 2) {
            if (bytes[i] == 0) {
                zerosEven++;
            }
            if (bytes[i + 1] == 0) {
                zerosOdd++;
            }
        }

        int max = Math.max(zerosEven, zerosOdd);
        return max >= pairs / 3;
    }

    private static boolean startsWithSensitivePrefix(String value) {
        String upper = value.toUpperCase(Locale.ROOT);
        for (String prefix : SENSITIVE_PREFIXES) {
            if (upper.startsWith(prefix)) {
                return true;
            }
        }

        int equals = value.indexOf('=');
        if (equals > 0) {
            String key = value.substring(0, equals).toLowerCase(Locale.ROOT);
            if (key.startsWith("user.") || key.startsWith("os.")) {
                return true;
            }
        }

        return false;
    }

    private static String redactWholeMatches(String input, Pattern pattern) {
        Matcher matcher = pattern.matcher(input);
        StringBuffer output = new StringBuffer();
        while (matcher.find()) {
            matcher.appendReplacement(output, Matcher.quoteReplacement(maskKeepingShape(matcher.group())));
        }
        matcher.appendTail(output);
        return output.toString();
    }

    private static String redactMatchGroup(String input, Pattern pattern, int groupIndex) {
        Matcher matcher = pattern.matcher(input);
        StringBuffer output = new StringBuffer();
        while (matcher.find()) {
            String full = matcher.group(0);
            String group = matcher.group(groupIndex);
            String masked = maskKeepingShape(group);
            String replaced = full.replace(group, masked);
            matcher.appendReplacement(output, Matcher.quoteReplacement(replaced));
        }
        matcher.appendTail(output);
        return output.toString();
    }

    private static String maskAfterEquals(String value) {
        int equals = value.indexOf('=');
        if (equals < 0 || equals == value.length() - 1) {
            return value;
        }
        String key = value.substring(0, equals + 1);
        String secret = value.substring(equals + 1);
        return key + maskKeepingShape(secret);
    }

    private static String maskKeepingShape(String value) {
        char[] chars = value.toCharArray();
        for (int i = 0; i < chars.length; i++) {
            if (!Character.isWhitespace(chars[i])) {
                chars[i] = '*';
            }
        }
        return new String(chars);
    }
}
