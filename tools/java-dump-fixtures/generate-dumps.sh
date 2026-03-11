#!/usr/bin/env bash
set -euo pipefail

print_help() {
  cat <<'EOF'
Usage:
  tools/java-dump-fixtures/generate-dumps.sh <mode> [hold_seconds] [profile_set] [truncate_bytes] [scenario] [sanitize]
  tools/java-dump-fixtures/generate-dumps.sh [options]

Arguments:
  mode           auto | manual | both
  hold_seconds   default: 120
  profile_set    standard | all | ultra   (default: standard)
  truncate_bytes default: 0
  scenario       01 | 02 | 03 | 04 | 05 | all   (default: 01)
  sanitize       off | on | only   (default: off)

Options:
  -m, --mode <value>
  -H, --hold-seconds <value>
  -p, --profile-set <value>
  -t, --truncate-bytes <value>
  -s, --scenario <value>
  -S, --sanitize <value>
  -h, --help

Examples:
  tools/java-dump-fixtures/generate-dumps.sh auto
  tools/java-dump-fixtures/generate-dumps.sh both 180 all 4194304
  tools/java-dump-fixtures/generate-dumps.sh auto 120 ultra 2097152 01
  tools/java-dump-fixtures/generate-dumps.sh auto 120 standard 0 all
  tools/java-dump-fixtures/generate-dumps.sh --mode auto --profile-set ultra --scenario 01 --sanitize on
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

if [[ "${SANITIZE}" != "off" && ! -x "${REDACT_SCRIPT}" ]]; then
  echo "[heap-fixture] sanitizer script not found or not executable: ${REDACT_SCRIPT}" >&2
  exit 1
fi

if [[ "${SCENARIO}" == "all" ]]; then
  scenarios=(01 02 03 04 05)
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
else
  echo "[heap-fixture] invalid scenario '${SCENARIO}' (expected: 01|02|03|04|05|all)" >&2
  exit 1
fi

if [[ "${PROFILE_SET}" == "standard" ]]; then
  profiles=(tiny medium large xlarge)
elif [[ "${PROFILE_SET}" == "all" ]]; then
  profiles=(tiny medium large xlarge ultra)
elif [[ "${PROFILE_SET}" == "ultra" ]]; then
  profiles=(ultra)
else
  echo "[heap-fixture] invalid profile_set '${PROFILE_SET}' (expected: standard|all|ultra)" >&2
  exit 1
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
    if [[ "${dump}" == *"-sanitized.hprof" || "${dump}" == *"-sanitized-"*".hprof" ]]; then
      continue
    fi
    local out="${dump%.hprof}-sanitized.hprof"
    echo "[heap-fixture] sanitize input=${dump} output=${out}"
    "${REDACT_SCRIPT}" "${dump}" "${out}"
  done
}

for profile in "${profiles[@]}"; do
  for scenario in "${scenarios[@]}"; do
    output="${ASSETS_DIR}/fixture-s${scenario}-${profile}.hprof"
    if [[ "${SANITIZE}" != "only" ]]; then
      echo "[heap-fixture] scenario=${scenario} profile=${profile} mode=${MODE} output=${output} truncateBytes=${TRUNCATE_BYTES}"
      java -cp "${CLASS_DIR}" HeapDumpFixture \
        --scenario "${scenario}" \
        --profile "${profile}" \
        --dump-mode "${MODE}" \
        --hold-seconds "${HOLD_SECONDS}" \
        --truncate-bytes "${TRUNCATE_BYTES}" \
        --output "${output}"
    fi

    if [[ "${SANITIZE}" == "on" || "${SANITIZE}" == "only" ]]; then
      sanitize_prefix "${output%.hprof}"
    fi
  done
done

echo "[heap-fixture] done"
