# hprof-redact custom transformer

Custom runner built on top of `hprof-redact` to redact sensitive path-like values while preserving most heap readability.

## What it redacts

- Windows paths (`C:\...`)
- UNC paths (`\\server\share\...`)
- Unix paths (`/home/...`, `/Users/...`, `/usr/...`, etc.)
- System property values for `user.*` and `os.*` keys
- JVM path args (`-javaagent:...`, `-Xbootclasspath:...`, `-Dfoo=/path/...`)
- Common env var values (`PATH=...`, `JAVA_HOME=...`, `USERPROFILE=...`, etc.)

Masking behavior:

- Non-whitespace characters in matched sensitive values are replaced with `*`
- This includes path separators (`/` and `\`) and punctuation

## What it preserves

- Java symbol-like UTF-8 strings (class names/signatures)
- Primitive values (this transformer is string/path focused)

## Build

```bash
mvn -q -f tools/hprof-redact-custom/pom.xml -DskipTests package
```

Output jar:

```text
tools/hprof-redact-custom/target/hprof-path-redact.jar
```

## Run

```bash
java -jar tools/hprof-redact-custom/target/hprof-path-redact.jar input.hprof output-redacted.hprof
```

Or via wrappers (auto-help if no args):

- Bash: `tools/hprof-redact-custom/redact.sh`
- PowerShell: `./tools/hprof-redact-custom/redact.ps1`
- CMD: `tools\hprof-redact-custom\redact.cmd`

## Notes

- This depends on `me.bechberger:hprof-redact:0.2.1`.
- Always validate a sample redacted dump in `hprof-visualizer` before bulk processing.
- If output already exists and cannot be overwritten (file lock), the runner writes to a fallback file with `-1`, `-2`, etc.
