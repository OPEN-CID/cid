#!/usr/bin/env bash
# Backs up a running cid-core's SQLite database safely.
#
# docs/021-Storage.md documents that no backup/restore mechanism exists in
# Core itself — this script is the operational answer. cid-core runs its
# database in WAL mode (see CLAUDE.md's "Known Windows-specific operational
# issues" for why that mode was chosen), so a plain `cp` of the .db file
# while Core is running can miss committed data still sitting in the
# -wal file. `sqlite3 .backup` takes a consistent snapshot through SQLite's
# own backup API and is safe to run against a live database.
#
# Usage: ./backup-cid-db.sh /path/to/cid.db /path/to/backup/dir
#
# Restore: stop cid-core, then copy the chosen backup file over the real
# database path (and delete any stale cid.db-wal/cid.db-shm next to it before
# starting Core again), then restart.

set -euo pipefail

DB_PATH="${1:?Usage: $0 <path-to-cid.db> <backup-dir>}"
BACKUP_DIR="${2:?Usage: $0 <path-to-cid.db> <backup-dir>}"

if ! command -v sqlite3 >/dev/null 2>&1; then
    echo "sqlite3 CLI not found — install it (e.g. apt install sqlite3 / brew install sqlite)" >&2
    exit 1
fi

mkdir -p "$BACKUP_DIR"
TIMESTAMP="$(date -u +%Y%m%dT%H%M%SZ)"
DEST="$BACKUP_DIR/cid-$TIMESTAMP.db"

sqlite3 "$DB_PATH" ".backup '$DEST'"
echo "Backed up $DB_PATH -> $DEST"

# Keep the last 14 backups only; adjust retention to your own policy.
ls -1t "$BACKUP_DIR"/cid-*.db 2>/dev/null | tail -n +15 | xargs -r rm --
