#!/usr/bin/env bash
set -euo pipefail

print_help() {
  cat <<'EOF'
Usage:
  tools/java-dump-fixtures/generate-dumps.sh <mode> [hold_seconds] [profile_set] [truncate_bytes] [scenario] [sanitize] [truncate_target] [remove_raw]
  tools/java-dump-fixtures/generate-dumps.sh [options]

Arguments:
  mode           auto | manual | both
  hold_seconds   default: 120
  profile_set    standard | all | tiny | medium | large | xlarge | ultra   (default: standard)
  truncate_bytes default: 0
  scenario       01 | 02 | ... | 10 | 11 | all   (default: 01)
  sanitize       off | on | only   (default: off)
  truncate_target raw | sanitized | both   (default: raw)
  remove_raw     off | on   (default: off)

Options:
  -m, --mode <value>
  -H, --hold-seconds <value>
  -p, --profile-set <value>
  -t, --truncate-bytes <value>
  -s, --scenario <value>
  -S, --sanitize <value>
  -T, --truncate-target <value>
  -R, --remove-raw <value>
  -h, --help

Examples:
  tools/java-dump-fixtures/generate-dumps.sh auto
  tools/java-dump-fixtures/generate-dumps.sh both 180 all 4194304
  tools/java-dump-fixtures/generate-dumps.sh auto 120 ultra 2097152 01
  tools/java-dump-fixtures/generate-dumps.sh auto 120 standard 0 all
  tools/java-dump-fixtures/generate-dumps.sh --mode auto --profile-set ultra --scenario 01 --sanitize on --truncate-target both --remove-raw on
EOF
}

if [[ $# -eq 0 ]]; then
  print_help
  exit 0
fi

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" || "${1:-}" == "help" ]]; then
  print_help
  exit 0
fi

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
CLASS_DIR="${SCRIPT_DIR}/out"
ASSETS_DIR="${SCRIPT_DIR}/../../assets/generated"

MODE="auto"
HOLD_SECONDS="120"
PROFILE_SET="standard"
TRUNCATE_BYTES="0"
SCENARIO="01"
SANITIZE="off"
TRUNCATE_TARGET="raw"
REMOVE_RAW="off"

POSITIONAL_INDEX=1
while [[ $# -gt 0 ]]; do
  case "$1" in
    -m|--mode)
      MODE="${2:-}"
      shift 2
      ;;
    -H|--hold-seconds)
      HOLD_SECONDS="${2:-}"
      shift 2
      ;;
    -p|--profile-set)
      PROFILE_SET="${2:-}"
      shift 2
      ;;
    -t|--truncate-bytes)
      TRUNCATE_BYTES="${2:-}"
      shift 2
      ;;
    -s|--scenario)
      SCENARIO="${2:-}"
      shift 2
      ;;
    -S|--sanitize)
      SANITIZE="${2:-}"
      shift 2
      ;;
    -T|--truncate-target)
      TRUNCATE_TARGET="${2:-}"
      shift 2
      ;;
    -R|--remove-raw)
      REMOVE_RAW="${2:-}"
      shift 2
      ;;
    -h|--help)
      print_help
      exit 0
      ;;
    --)
      shift
      while [[ $# -gt 0 ]]; do
        case "${POSITIONAL_INDEX}" in
          1) MODE="$1" ;;
          2) HOLD_SECONDS="$1" ;;
          3) PROFILE_SET="$1" ;;
          4) TRUNCATE_BYTES="$1" ;;
          5) SCENARIO="$1" ;;
          6) SANITIZE="$1" ;;
          7) TRUNCATE_TARGET="$1" ;;
          8) REMOVE_RAW="$1" ;;
          *)
            echo "[heap-fixture] too many positional arguments" >&2
            print_help
            exit 1
            ;;
        esac
        POSITIONAL_INDEX=$((POSITIONAL_INDEX + 1))
        shift
      done
      ;;
    -*)
      echo "[heap-fixture] unknown option '$1'" >&2
      print_help
      exit 1
      ;;
    *)
      case "${POSITIONAL_INDEX}" in
        1) MODE="$1" ;;
        2) HOLD_SECONDS="$1" ;;
        3) PROFILE_SET="$1" ;;
        4) TRUNCATE_BYTES="$1" ;;
        5) SCENARIO="$1" ;;
        6) SANITIZE="$1" ;;
        7) TRUNCATE_TARGET="$1" ;;
        8) REMOVE_RAW="$1" ;;
        *)
          echo "[heap-fixture] too many positional arguments" >&2
          print_help
          exit 1
          ;;
      esac
      POSITIONAL_INDEX=$((POSITIONAL_INDEX + 1))
      shift
      ;;
  esac
done

REDACT_SCRIPT="${SCRIPT_DIR}/../hprof-redact-custom/redact.sh"

if [[ "${MODE}" != "auto" && "${MODE}" != "manual" && "${MODE}" != "both" ]]; then
  echo "[heap-fixture] invalid mode '${MODE}' (expected: auto|manual|both)" >&2
  print_help
  exit 1
fi

if [[ "${SANITIZE}" != "off" && "${SANITIZE}" != "on" && "${SANITIZE}" != "only" ]]; then
  echo "[heap-fixture] invalid sanitize '${SANITIZE}' (expected: off|on|only)" >&2
  exit 1
fi

if [[ "${TRUNCATE_TARGET}" != "raw" && "${TRUNCATE_TARGET}" != "sanitized" && "${TRUNCATE_TARGET}" != "both" ]]; then
  echo "[heap-fixture] invalid truncate_target '${TRUNCATE_TARGET}' (expected: raw|sanitized|both)" >&2
  exit 1
fi

if [[ "${REMOVE_RAW}" != "off" && "${REMOVE_RAW}" != "on" ]]; then
  echo "[heap-fixture] invalid remove_raw '${REMOVE_RAW}' (expected: off|on)" >&2
  exit 1
fi

if [[ "${TRUNCATE_TARGET}" != "raw" && "${TRUNCATE_BYTES}" == "0" ]]; then
  echo "[heap-fixture] truncate_target '${TRUNCATE_TARGET}' requires truncate_bytes > 0" >&2
  exit 1
fi

if [[ "${SANITIZE}" == "off" && "${TRUNCATE_TARGET}" != "raw" ]]; then
  echo "[heap-fixture] truncate_target '${TRUNCATE_TARGET}' requires sanitize on or only" >&2
  exit 1
fi

if [[ "${REMOVE_RAW}" == "on" && "${SANITIZE}" != "on" ]]; then
  echo "[heap-fixture] remove_raw 'on' requires sanitize=on" >&2
  exit 1
fi

if [[ "${SANITIZE}" != "off" && ! -x "${REDACT_SCRIPT}" ]]; then
  echo "[heap-fixture] sanitizer script not found or not executable: ${REDACT_SCRIPT}" >&2
  exit 1
fi

if [[ "${SCENARIO}" == "all" ]]; then
  scenarios=(01 02 03 04 05 06 07 08 09 10 11)
elif [[ "${SCENARIO}" == "1" || "${SCENARIO}" == "01" ]]; then
  scenarios=(01)
elif [[ "${SCENARIO}" == "2" || "${SCENARIO}" == "02" ]]; then
  scenarios=(02)
elif [[ "${SCENARIO}" == "3" || "${SCENARIO}" == "03" ]]; then
  scenarios=(03)
elif [[ "${SCENARIO}" == "4" || "${SCENARIO}" == "04" ]]; then
  scenarios=(04)
elif [[ "${SCENARIO}" == "5" || "${SCENARIO}" == "05" ]]; then
  scenarios=(05)
elif [[ "${SCENARIO}" == "6" || "${SCENARIO}" == "06" ]]; then
  scenarios=(06)
elif [[ "${SCENARIO}" == "7" || "${SCENARIO}" == "07" ]]; then
  scenarios=(07)
elif [[ "${SCENARIO}" == "8" || "${SCENARIO}" == "08" ]]; then
  scenarios=(08)
elif [[ "${SCENARIO}" == "9" || "${SCENARIO}" == "09" ]]; then
  scenarios=(09)
elif [[ "${SCENARIO}" == "10" ]]; then
  scenarios=(10)
elif [[ "${SCENARIO}" == "11" ]]; then
  scenarios=(11)
else
  echo "[heap-fixture] invalid scenario '${SCENARIO}' (expected: 01|02|...|10|11|all)" >&2
  exit 1
fi

case "${PROFILE_SET}" in
  standard) profiles=(tiny medium large xlarge) ;;
  all)      profiles=(tiny medium large xlarge ultra) ;;
  tiny)     profiles=(tiny) ;;
  medium)   profiles=(medium) ;;
  large)    profiles=(large) ;;
  xlarge)   profiles=(xlarge) ;;
  ultra)    profiles=(ultra) ;;
  *)
    echo "[heap-fixture] invalid profile_set '${PROFILE_SET}' (expected: standard|all|tiny|medium|large|xlarge|ultra)" >&2
    exit 1
    ;;
esac

if [[ "${SCENARIO}" == "11" ]]; then
  for p in "${profiles[@]}"; do
    case "${p}" in
      xlarge) echo "[heap-fixture] WARNING: S11 xlarge requires ~20 GB free RAM, produces ~12 GB dump" >&2 ;;
      ultra)  echo "[heap-fixture] WARNING: S11 ultra requires ~28 GB free RAM, produces ~20 GB dump" >&2 ;;
    esac
  done
fi

mkdir -p "${CLASS_DIR}"
mkdir -p "${ASSETS_DIR}"

sources=("${SCRIPT_DIR}/HeapDumpFixture.java")
for src in "${SCRIPT_DIR}"/support/*.java; do
  sources+=("${src}")
done
for src in "${SCRIPT_DIR}"/scenarios/*.java; do
  sources+=("${src}")
done

if [[ "${SANITIZE}" != "only" ]]; then
  javac -d "${CLASS_DIR}" "${sources[@]}"
fi

sanitize_prefix() {
  local prefix="$1"
  shopt -s nullglob
  local dumps=("${prefix}"*.hprof)
  shopt -u nullglob

  for dump in "${dumps[@]}"; do
    if [[ "${dump}" == *"-san.hprof" || "${dump}" == *"-san-"*".hprof" || "${dump}" == *"-sanitized.hprof" || "${dump}" == *"-sanitized-"*".hprof" ]]; then
      continue
    fi
    if [[ "${dump}" == *"-truncated.hprof" || "${dump}" == *"-truncated-"*".hprof" ]]; then
      echo "[heap-fixture] sanitize skip truncated=${dump}"
      continue
    fi
    local out
    if [[ "${dump}" == *"-raw.hprof" ]]; then
      out="${dump%-raw.hprof}-san.hprof"
    else
      out="${dump%.hprof}-san.hprof"
    fi
    echo "[heap-fixture] sanitize input=${dump} output=${out}"
    "${REDACT_SCRIPT}" "${dump}" "${out}"
  done
}

truncate_file() {
  local input="$1"
  local bytes_to_remove="$2"
  local output="${input%.hprof}-truncated.hprof"

  if [[ ! -f "${input}" ]]; then
    return
  fi

  python3 - <<'PY' "$input" "$output" "$bytes_to_remove"
import os
import sys

input_path = sys.argv[1]
output_path = sys.argv[2]
remove = int(sys.argv[3])

size = os.path.getsize(input_path)
keep = size - remove
if keep < 1:
    keep = 1

if os.path.exists(output_path):
    os.remove(output_path)

with open(input_path, 'rb') as src, open(output_path, 'wb') as dst:
    remaining = keep
    while remaining > 0:
        chunk = src.read(min(8192, remaining))
        if not chunk:
            break
        dst.write(chunk)
        remaining -= len(chunk)

print(f"truncatedDumpPath={output_path} original={size} truncated={os.path.getsize(output_path)}")
PY
}

truncate_sanitized_prefix() {
  local prefix="$1"
  local bytes_to_remove="$2"
  local base_prefix="${prefix%-raw}"
  shopt -s nullglob
  local dumps=("${base_prefix}"*-san.hprof "${base_prefix}"*-sanitized.hprof)
  shopt -u nullglob

  for dump in "${dumps[@]}"; do
    echo "[heap-fixture] truncate sanitized input=${dump}"
    truncate_file "${dump}" "${bytes_to_remove}"
  done
}

remove_raw_prefix() {
  local prefix="$1"
  shopt -s nullglob
  local dumps=("${prefix}"*.hprof)
  shopt -u nullglob

  for dump in "${dumps[@]}"; do
    if [[ "${dump}" == *"-san.hprof" || "${dump}" == *"-san-"*".hprof" || "${dump}" == *"-sanitized.hprof" || "${dump}" == *"-sanitized-"*".hprof" ]]; then
      continue
    fi
    echo "[heap-fixture] remove raw=${dump}"
    rm -f "${dump}"
  done
}

jvm_heap_for_profile() {
  local profile="$1"
  local scenario="${2:-}"
  if [[ "${scenario}" == "11" ]]; then
    case "${profile}" in
      tiny)   echo "2g" ;;
      medium) echo "4g" ;;
      large)  echo "10g" ;;
      xlarge) echo "20g" ;;
      ultra)  echo "28g" ;;
      *)      echo "4g" ;;
    esac
    return
  fi
  case "${profile}" in
    tiny)     echo "512m" ;;
    medium)   echo "768m" ;;
    large)    echo "1g" ;;
    xlarge)   echo "2g" ;;
    ultra)    echo "4g" ;;
    *)        echo "1g" ;;
  esac
}

for profile in "${profiles[@]}"; do
  for scenario in "${scenarios[@]}"; do
    output="${ASSETS_DIR}/fixture-s${scenario}-${profile}-raw.hprof"
    if [[ "${SANITIZE}" != "only" ]]; then
      truncate_for_java="${TRUNCATE_BYTES}"
      if [[ "${TRUNCATE_TARGET}" == "sanitized" ]]; then
        truncate_for_java="0"
      fi

      xmx="$(jvm_heap_for_profile "${profile}" "${scenario}")"
      echo "[heap-fixture] scenario=${scenario} profile=${profile} mode=${MODE} output=${output} truncateBytes=${TRUNCATE_BYTES} -Xmx${xmx}"
      java -Xmx"${xmx}" -cp "${CLASS_DIR}" HeapDumpFixture \
        --scenario "${scenario}" \
        --profile "${profile}" \
        --dump-mode "${MODE}" \
        --hold-seconds "${HOLD_SECONDS}" \
        --truncate-bytes "${truncate_for_java}" \
        --output "${output}"
    fi

    if [[ "${SANITIZE}" == "on" || "${SANITIZE}" == "only" ]]; then
      sanitize_prefix "${output%.hprof}"

      if [[ "${TRUNCATE_BYTES}" != "0" && ( "${TRUNCATE_TARGET}" == "sanitized" || "${TRUNCATE_TARGET}" == "both" ) ]]; then
        truncate_sanitized_prefix "${output%.hprof}" "${TRUNCATE_BYTES}"
      fi

      if [[ "${REMOVE_RAW}" == "on" ]]; then
        remove_raw_prefix "${output%.hprof}"
      fi
    fi
  done
done

echo "[heap-fixture] done"
