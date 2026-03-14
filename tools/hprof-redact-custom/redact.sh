#!/usr/bin/env bash
set -euo pipefail

print_help() {
  cat <<'EOF'
Usage:
  tools/hprof-redact-custom/redact.sh <input.hprof> <output.hprof>

Description:
  Builds and runs the custom path-focused HPROF redactor.

Examples:
  tools/hprof-redact-custom/redact.sh assets/generated/fixture-s01-ultra-auto.hprof assets/generated/fixture-s01-ultra-auto-redacted.hprof
EOF
}

if [[ $# -eq 0 || "${1:-}" == "-h" || "${1:-}" == "--help" || "${1:-}" == "help" ]]; then
  print_help
  exit 0
fi

if [[ $# -ne 2 ]]; then
  echo "Invalid arguments." >&2
  print_help
  exit 1
fi

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
POM_PATH="${SCRIPT_DIR}/pom.xml"

mvn -q -f "${POM_PATH}" -DskipTests package
java -jar "${SCRIPT_DIR}/target/hprof-path-redact.jar" "$1" "$2"
