#!/usr/bin/env python3
"""
redact_hprof.py — Supprime les strings sensibles (env vars, JVM args) d'un heap dump HPROF.

Usage:
    python3 redact_hprof.py <input.hprof> [output.hprof]

    Si output.hprof est omis, le fichier est modifié IN-PLACE (une sauvegarde .bak est créée).

Personnalisation:
    Modifie les listes SENSITIVE_PATTERNS et SENSITIVE_PREFIXES ci-dessous
    pour adapter aux données à supprimer.
"""

import sys
import os
import re
import shutil
import struct
from pathlib import Path

# ─────────────────────────────────────────────
#  CONFIGURATION — adapte ces patterns à ton cas
# ─────────────────────────────────────────────

# Patterns regex : chemins système uniquement.
#
# Les noms de classes Java en bytecode contiennent '/' mais ne commencent
# jamais par des segments OS réels (/home/, /Users/, C:\, etc.).
# On cible uniquement des préfixes de chemins système reconnaissables.

# Préfixes de chemins Unix qui ne sont jamais des packages Java
_UNIX_PATH_ROOTS = (
    r'/home/', r'/Users/', r'/opt/', r'/usr/local/', r'/var/',
    r'/tmp/', r'/etc/', r'/srv/', r'/mnt/', r'/media/', r'/root/',
    r'/private/', r'/Library/',
)
_UNIX_ROOT_RE = '|'.join(re.escape(p) for p in _UNIX_PATH_ROOTS)

# Chaque pattern est un tuple (regex, group_to_redact).
# group_to_redact=0 → redacter le match complet
# group_to_redact=1 → redacter seulement le groupe capturant 1

_PATH_CHARS = r'[\w.\- /\\:]{4,300}'
# (?<!\w) : le C de C:\ ne doit pas être précédé d'un autre mot-caractère
# → exclut http:// (le 'p' est précédé de 'tt') mais accepte 'C:\' seul
_WIN_PATH   = r'(?<!\w)[A-Za-z]:[/\\]' + _PATH_CHARS
# (?<!\w) : le '/' d'un chemin unix ne doit pas être précédé d'un mot-caractère
# → exclut les segments Java comme 'com/sun/media/...' (le '/' précédé par 'n')
_UNIX_PATH  = r'(?<!\w)(?:' + _UNIX_ROOT_RE + r')' + _PATH_CHARS

SENSITIVE_PATTERNS: list[tuple[str, int]] = [
    # Chemin Unix absolu débutant par un répertoire système connu
    (_UNIX_PATH, 0),
    # Chemin Windows absolu   ex: C:\Users\florian\...
    (_WIN_PATH, 0),
    # JVM -D dont la valeur est un chemin   ex: -Duser.home=/home/florian
    (r'-D[\w.]{1,60}=(' + _UNIX_PATH + r'|' + _WIN_PATH + r')', 1),
    # -javaagent/-agentlib/-agentpath avec chemin
    (r'-(?:javaagent|agentlib|agentpath):([\w.\-/\\:=,]{4,300})', 1),
    # -Xbootclasspath avec chemin
    (r'-Xbootclasspath(?:/p)?:([\w.\-/\\:]{4,300})', 1),
]

# Préfixes exacts : toute string commençant par l'un de ceux-ci sera écrasée
# (la valeur entière est redactée, donc doit vraiment être une clé=valeur complète)
SENSITIVE_PREFIXES = [
    "JAVA_HOME=",
    "JRE_HOME=",
    "JDK_HOME=",
    "PATH=",
    "LD_LIBRARY_PATH=",
    "CLASSPATH=",
    "CATALINA_HOME=",
    "CATALINA_BASE=",
    "APP_HOME=",
    "HOME=",
    "TMPDIR=",
    "TEMP=",
    "TMP=",
    "USERPROFILE=",
    "APPDATA=",
    "LOCALAPPDATA=",
    "PROGRAMFILES=",
]

REDACT_CHAR = ord('*')  # Caractère de remplacement (en bytes)

# ─────────────────────────────────────────────
#  FORMAT HPROF
# ─────────────────────────────────────────────
# Header : <string NUL-terminée> + u32 id_size + u64 timestamp
# Records: u8 tag + u32 time_offset + u32 length + <body>
# Tag 0x01 = HPROF_UTF8 : u<id_size> identifier + <length - id_size> octets UTF-8

TAG_UTF8 = 0x01

# ─────────────────────────────────────────────
#  LOGIQUE PRINCIPALE
# ─────────────────────────────────────────────

compiled_patterns = [(re.compile(pat), grp) for pat, grp in SENSITIVE_PATTERNS]

# Strings qui ressemblent entièrement à des noms de classes / signatures Java
# bytecode — on les ignore complètement pour éviter les faux positifs.
_JAVA_CLASS_RE = re.compile(
    r'^(?:'
    r'\[*L[\w/$]+;'              # [Ljava/util/List; ou Ljava/lang/Object;
    r'|[\w/$]+(?:/[\w/$]+)+'     # java/util/Arrays ou jdk/internal/...
    r'|\(.*\)[A-Z\[V]'           # (Ljava/lang/String;)V  signatures
    r'|<.*>'                     # <T:Ljava/lang/Object;>...
    r')$'
)


def sensitive_spans(s: str) -> list[tuple[int, int]]:
    """Retourne la liste des (start, end) en caractères à redacter dans s."""
    spans = []
    # Préfixes : toute la string est sensible
    for prefix in SENSITIVE_PREFIXES:
        if s.startswith(prefix):
            spans.append((0, len(s)))
            return spans
    # Patterns : redacter le groupe capturant ou le match complet
    for pat, grp in compiled_patterns:
        for m in pat.finditer(s):
            if grp and m.lastindex and m.lastindex >= grp:
                spans.append((m.start(grp), m.end(grp)))
            else:
                spans.append((m.start(), m.end()))
    return spans


def parse_header(data: bytes) -> tuple[int, int]:
    """Retourne (id_size, offset_après_header)."""
    nul = data.index(b'\x00')
    offset = nul + 1
    id_size = struct.unpack_from('>I', data, offset)[0]
    offset += 4 + 8  # id_size (u32) + timestamp (u64)
    return id_size, offset


def redact_hprof(src: Path, dst: Path) -> dict:
    stats = {"scanned": 0, "redacted": 0, "bytes_zeroed": 0}

    print(f"[+] Lecture de {src} ({src.stat().st_size:,} octets)...")
    data = bytearray(src.read_bytes())

    id_size, offset = parse_header(data)
    print(f"[+] id_size={id_size}, début des records à l'offset {offset}")

    total = len(data)
    while offset < total:
        if offset + 9 > total:
            break

        tag = data[offset]
        # time_offset = struct.unpack_from('>I', data, offset + 1)[0]  # non utilisé
        rec_len = struct.unpack_from('>I', data, offset + 5)[0]
        body_start = offset + 9
        body_end = body_start + rec_len

        if body_end > total:
            print(f"[!] Record tronqué à l'offset {offset}, arrêt.")
            break

        if tag == TAG_UTF8 and rec_len > id_size:
            str_offset = body_start + id_size
            str_len = rec_len - id_size
            raw = bytes(data[str_offset: str_offset + str_len])

            stats["scanned"] += 1
            try:
                decoded = raw.decode("utf-8", errors="replace")
            except Exception:
                decoded = ""

            spans = (decoded
                     and not _JAVA_CLASS_RE.match(decoded)
                     and sensitive_spans(decoded))
            if spans:
                # Convertit les spans char → spans byte et écrase in-place
                encoded = decoded.encode("utf-8")
                redacted_bytes = 0
                for char_start, char_end in spans:
                    byte_start = len(decoded[:char_start].encode("utf-8"))
                    byte_end = len(decoded[:char_end].encode("utf-8"))
                    for i in range(str_offset + byte_start,
                                   str_offset + byte_end):
                        data[i] = REDACT_CHAR
                    redacted_bytes += byte_end - byte_start
                stats["redacted"] += 1
                stats["bytes_zeroed"] += redacted_bytes
                redacted_preview = decoded
                for char_start, char_end in sorted(spans, reverse=True):
                    redacted_preview = (
                        redacted_preview[:char_start]
                        + "*" * (char_end - char_start)
                        + redacted_preview[char_end:]
                    )
                print(f"  [-] Original : {decoded[:120]}")
                print(f"      Rédacté  : {redacted_preview[:120]}")

        offset = body_end

    print(f"\n[+] Écriture de {dst}...")
    dst.write_bytes(data)
    return stats


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)

    src = Path(sys.argv[1])
    if not src.exists():
        print(f"[!] Fichier introuvable : {src}")
        sys.exit(1)

    if len(sys.argv) >= 3:
        dst = Path(sys.argv[2])
        in_place = False
    else:
        # Mode in-place : sauvegarde d'abord
        backup = src.with_suffix(src.suffix + ".bak")
        print(f"[+] Sauvegarde → {backup}")
        shutil.copy2(src, backup)
        dst = src
        in_place = True

    stats = redact_hprof(src if not in_place else Path(str(src) + ".bak"), dst)

    print("\n── Résumé ─────────────────────────────────")
    print(f"  Strings scannées : {stats['scanned']:,}")
    print(f"  Strings rédactées: {stats['redacted']:,}")
    print(f"  Octets écrasés   : {stats['bytes_zeroed']:,}")
    print(f"  Fichier de sortie: {dst}")
    print("────────────────────────────────────────────")


if __name__ == "__main__":
    main()
