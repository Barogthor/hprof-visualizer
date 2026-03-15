# Java heap dump fixtures

Multi-scenario Java generator to produce `.hprof` dumps with:

- primitives and primitive arrays
- primitive matrices (`int[][]`, `double[][][]`)
- enums
- custom types (with and without static fields)
- object arrays (`Object[]`, `Custom[]`)
- boxed primitive collections (`Integer`, `Long`, etc.) and custom object collections
- collections larger than 10k
- one 500k-item collection (profile `xlarge`)
- direct and indirect object cycles (1, 2, and 3 levels)
- ultra heavy profile with 1,000,000-item collection (`ultra`)
- optional intentionally truncated dump copies

## Files

- `tools/java-dump-fixtures/HeapDumpFixture.java`
- `tools/java-dump-fixtures/scenarios/Scenario01StackFrameTypes.java`
- `tools/java-dump-fixtures/scenarios/Scenario02CollectionsTopology.java`
- `tools/java-dump-fixtures/scenarios/Scenario03LeakPatterns.java`
- `tools/java-dump-fixtures/scenarios/Scenario04ReferenceTypes.java`
- `tools/java-dump-fixtures/scenarios/Scenario05HugeObjects.java`
- `tools/java-dump-fixtures/scenarios/Scenario06Deadlock.java`
- `tools/java-dump-fixtures/scenarios/Scenario07ThreadLocalPoolLeak.java`
- `tools/java-dump-fixtures/scenarios/Scenario08ClassLoaderRetention.java`
- `tools/java-dump-fixtures/scenarios/Scenario09ConcurrentMapHotBuckets.java`
- `tools/java-dump-fixtures/scenarios/Scenario10StringExtremes.java`
- `tools/java-dump-fixtures/support/*.java`
- `tools/java-dump-fixtures/generate-dumps.sh`
- `tools/java-dump-fixtures/generate-dumps.ps1`
- `tools/java-dump-fixtures/generate-dumps.cmd`

## Quick start

If you run any wrapper script without arguments, it now prints help/usage.

Generate scenario 01 with automatic dump only:

```bash
tools/java-dump-fixtures/generate-dumps.sh auto
```

Equivalent named options:

```bash
tools/java-dump-fixtures/generate-dumps.sh --mode auto --scenario 01
```

With automatic sanitization of generated dumps:

```bash
tools/java-dump-fixtures/generate-dumps.sh --mode auto --scenario 01 --sanitize on
```

With raw cleanup (keep only sanitized outputs):

```bash
tools/java-dump-fixtures/generate-dumps.sh --mode auto --scenario 01 --sanitize on --remove-raw on
```

Generate, sanitize, and also truncate sanitized dumps:

```bash
tools/java-dump-fixtures/generate-dumps.sh --mode auto --scenario 01 --truncate-bytes 1000 --sanitize on --truncate-target both
```

Sanitize only existing dumps (no generation):

```bash
tools/java-dump-fixtures/generate-dumps.sh --profile-set all --scenario all --sanitize only
```

Generate all scenarios for standard profiles with both auto dump and manual window (`jcmd` / `jmap`):

```bash
tools/java-dump-fixtures/generate-dumps.sh both 180 standard 0 all
```

Generate only ultra profile and create intentionally truncated copies (remove 4 MiB):

```bash
tools/java-dump-fixtures/generate-dumps.sh auto 120 ultra 4194304 01
```

Generate a ~20 GB dump (requires ~24 GB free RAM):

```bash
tools/java-dump-fixtures/generate-dumps.sh --mode auto --profile-set colossal --scenario 05
```

## Windows

PowerShell:

```powershell
./tools/java-dump-fixtures/generate-dumps.ps1 -Mode both -HoldSeconds 180 -ProfileSet all -TruncateBytes 4194304 -Scenario all
```

PowerShell with sanitization:

```powershell
./tools/java-dump-fixtures/generate-dumps.ps1 -Mode auto -ProfileSet standard -Scenario 01 -Sanitize on
```

PowerShell with raw cleanup:

```powershell
./tools/java-dump-fixtures/generate-dumps.ps1 -Mode auto -Scenario 01 -Sanitize on -RemoveRaw on
```

PowerShell with sanitized truncation:

```powershell
./tools/java-dump-fixtures/generate-dumps.ps1 -Mode auto -Scenario 01 -TruncateBytes 1000 -Sanitize on -TruncateTarget both
```

PowerShell sanitize-only mode:

```powershell
./tools/java-dump-fixtures/generate-dumps.ps1 -ProfileSet all -Scenario all -Sanitize only
```

CMD:

```cmd
tools\java-dump-fixtures\generate-dumps.cmd both 180 all 4194304 all
```

CMD with named options:

```cmd
tools\java-dump-fixtures\generate-dumps.cmd --mode both --hold-seconds 180 --profile-set all --truncate-bytes 4194304 --scenario all
```

CMD with sanitization:

```cmd
tools\java-dump-fixtures\generate-dumps.cmd --mode auto --profile-set standard --scenario 01 --sanitize on
```

CMD with raw cleanup:

```cmd
tools\java-dump-fixtures\generate-dumps.cmd --mode auto --scenario 01 --sanitize on --remove-raw on
```

CMD with sanitized truncation:

```cmd
tools\java-dump-fixtures\generate-dumps.cmd --mode auto --scenario 01 --truncate-bytes 1000 --sanitize on --truncate-target both
```

CMD sanitize-only mode:

```cmd
tools\java-dump-fixtures\generate-dumps.cmd --profile-set all --scenario all --sanitize only
```

Generate a ~20 GB dump (requires ~24 GB free RAM):

```powershell
./tools/java-dump-fixtures/generate-dumps.ps1 -Mode auto -ProfileSet colossal -Scenario 05
```

```cmd
tools\java-dump-fixtures\generate-dumps.cmd --mode auto --profile-set colossal --scenario 05
```

Show help explicitly:

```bash
tools/java-dump-fixtures/generate-dumps.sh --help
```

```powershell
./tools/java-dump-fixtures/generate-dumps.ps1 -Help
```

```cmd
tools\java-dump-fixtures\generate-dumps.cmd --help
```

## Run one profile manually

```bash
javac -d tools/java-dump-fixtures/out \
  tools/java-dump-fixtures/HeapDumpFixture.java \
  tools/java-dump-fixtures/support/*.java \
  tools/java-dump-fixtures/scenarios/*.java
java -cp tools/java-dump-fixtures/out HeapDumpFixture \
  --scenario 01 \
  --profile ultra \
  --dump-mode both \
  --hold-seconds 300 \
  --truncate-bytes 4194304 \
  --output assets/generated/fixture-s01-ultra.hprof
```

The program prints:

- `pid=<...>`
- one `jcmd` command
- one `jmap` command

Use those commands while the process is waiting.

## Profiles

- `tiny`, `medium`, `large`, `xlarge`, `ultra`, `colossal`
- all profiles include 10k+ collections
- `xlarge` includes one 500k-item wrapper collection
- `ultra` includes one 1,000,000-item wrapper collection
- `colossal` is locked to scenario 05 only (requires ~24 GB free RAM, produces ~20 GB dump)

Detailed profile sizing (from `ProfileSpec`):

| Profile | boxed/custom collections (each) | object/custom arrays | matrix | graph nodes | huge collection | frame-root objects | heavy blocks (`MiB x count`) |
|---|---:|---:|---:|---:|---:|---:|---:|
| `tiny` | 10,240 | 512 / 1,024 | 48x48 | 128 | 0 | 256 | 4 x 4 |
| `medium` | 14,336 | 1,024 / 2,048 | 64x64 | 256 | 0 | 512 | 8 x 6 |
| `large` | 20,480 | 2,048 / 4,096 | 96x96 | 512 | 0 | 1,024 | 12 x 8 |
| `xlarge` | 30,720 | 4,096 / 8,192 | 128x128 | 768 | 500,000 | 2,048 | 16 x 10 |
| `ultra` | 65,536 | 16,384 / 32,768 | 256x256 | 2,048 | 1,000,000 | 8,192 | 20 x 12 |
| `colossal` | 131,072 | 32,768 / 65,536 | 256x256 | 4,096 | 2,000,000 | 16,384 | 256 x 52 |

`profile-set` values in wrapper scripts:

- `standard`: `tiny`, `medium`, `large`, `xlarge`
- `all`: `tiny`, `medium`, `large`, `xlarge`, `ultra`
- `ultra`: `ultra` only
- `colossal`: `colossal` only

Indicative runtime envelope (order of magnitude):

| Profile | Suggested JVM heap (`-Xmx`) | Typical generated dump size (single scenario) | Typical auto dump time |
|---|---:|---:|---:|
| `tiny` | 512m | ~10-30 MB | < 1 s |
| `medium` | 768m | ~15-45 MB | < 1 s |
| `large` | 1g | ~20-80 MB | ~1 s |
| `xlarge` | 2g | ~50-250 MB | 1-5 s |
| `ultra` | 4g+ | ~150 MB to 1+ GB | 5-30 s |
| `colossal` | 24g+ | ~20 GB (scenario 05) | 1-5 min |

Notes on these estimates:

- They are highly dependent on JDK version, GC, machine speed, and selected scenario.
- `05` (huge objects), `07` (thread-local pool), `08` (classloader retention), and `10` (string extremes) are generally heavier than `01`.
- `sanitize=on` adds a full extra pass over each non-truncated dump, so wall time increases roughly with dump size.

## Scenarios

- `01`: stack frames + Java types/local variables (priority visual validation)
- `02`: collection topologies (nulls, collisions, shared refs)
- `03`: leak patterns (static cache, thread-local, classloader-like)
- `04`: reference types (weak/soft/phantom + queue)
- `05`: huge objects and large arrays
- `06`: intentional deadlock (2 threads, 2 monitors)
- `07`: thread-local retention on fixed thread pool
- `08`: classloader retention through static cache
- `09`: concurrent hash map with hot collision buckets
- `10`: string extremes (very long, utf variants, similar prefixes)

## Notes

- Output path defaults to `assets/generated/fixture-s<scenario>-<profile>.hprof`
- Wrapper scripts generate raw dumps with `-raw` suffix, e.g. `fixture-s01-medium-raw.hprof`
- In `both` mode, auto dump goes to `*-auto.hprof`
- Manual suggestions use `*-jcmd.hprof` and `*-jcmd-jmap.hprof`
- Existing target dump file is deleted before auto dump
- If `--truncate-bytes > 0`, a `*-truncated.hprof` copy is created from each produced dump
- `truncate-target` controls where truncation is applied:
  - `raw` (default): truncate raw dumps only
  - `sanitized`: truncate sanitized dumps only
  - `both`: truncate raw and sanitized dumps
- If `sanitize=on`, each produced dump gets a `*-san.hprof` companion file (e.g. `fixture-s01-medium-san.hprof`)
- If `sanitize=only`, the script skips generation and sanitizes matching existing dumps only
- Truncated dumps are skipped by sanitization (expected, because they are intentionally corrupted)
- If `remove_raw=on`, raw dumps (`*.hprof` without `-san`) are deleted after successful sanitization (requires `sanitize=on`)
