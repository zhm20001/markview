#!/usr/bin/env bash
# Allocate one of the eight pinned P-core slots and run a command in it.
#
# The host has four physical P-cores (4.5 GHz), hyperthreaded into eight
# logical CPUs: 0,1,2,3,4,5,6,7. Long-running work is capped at eight threads
# and pinned there, so the E-cores stay free for builds and triage.
#
#   slot.sh --list                 show which slots are held
#   slot.sh --wait <cmd...>        block for a slot, then run <cmd>
#   slot.sh --try  <cmd...>        run only if a slot is free right now
#
# The slot is held for the lifetime of the command by an flock on a lock file
# under the artifacts directory; a crashed holder releases it automatically.
# Set SLOT_RSS_MB to cap the child's resident size (default 4096 MiB).
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
lockdir="${FUZZ_SLOT_DIR:-$root/artifacts/fuzz-next/.slots}"
mkdir -p "$lockdir"

# One logical CPU per slot, all of them P-core threads. `0` pairs with `5`
# (same physical core) so each physical P-core carries at most two of the
# eight slots, which is exactly its hyperthread capacity.
declare -a SLOT_CPU=(1 2 3 4 6 7 0 5)
SLOTS=${#SLOT_CPU[@]}

usage() { sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }

list_slots() {
	local i
	for ((i = 0; i < SLOTS; i++)); do
		local f="$lockdir/slot$i.lock"
		local state=free holder=""
		if ! flock -n "$f" true 2>/dev/null; then
			# Held, so the file names the command that holds it.
			state=busy
			holder=$(head -c 200 "$f" 2>/dev/null | tr '\n' ' ' || true)
		fi
		printf 'slot%d cpu=%s %-4s %s\n' "$i" "${SLOT_CPU[$i]}" "$state" "$holder"
	done
}

run_in_slot() {
	local idx=$1; shift
	local cpu=${SLOT_CPU[$idx]}
	local lock="$lockdir/slot$idx.lock"
	local rss_mb=${SLOT_RSS_MB:-4096}
	# Record the holder for `--list`; the lock itself is what reserves it.
	printf '%s\n' "$$ $*" >"$lock"
	# ASAN reserves tens of terabytes of *virtual* address space for shadow
	# memory, so a `ulimit -v` cap kills every sanitized binary at startup.
	# The cap is therefore on resident size, applied by the watchdog below.
	exec taskset -c "$cpu" "$root/fuzz/scripts/rsscap.sh" "$rss_mb" "$@"
}

case "${1:-}" in
--list | list) list_slots ;;
--wait | wait)
	shift
	[[ $# -gt 0 ]] || usage
	# `--wait` blocks until a slot frees up; scanning once and exiting 75 is
	# `--try`'s contract. `SLOT_WAIT_SECS` spaces the scans so eight busy
	# slots cost one short sleep per round instead of a busy spin.
	while :; do
		for ((i = 0; i < SLOTS; i++)); do
			# Hold the lock for the whole command: flock keeps the fd open over
			# exec. Append rather than truncate so a held slot keeps the holder
			# record `--list` prints.
			exec 9>>"$lockdir/slot$i.lock"
			if flock -n 9; then
				printf 'slot%d cpu=%s\n' "$i" "${SLOT_CPU[$i]}" >&2
				run_in_slot "$i" "$@"
			fi
			exec 9>&-
		done
		sleep "${SLOT_WAIT_SECS:-5}"
	done
	;;
--try | try)
	shift
	[[ $# -gt 0 ]] || usage
	for ((i = 0; i < SLOTS; i++)); do
		exec 9>>"$lockdir/slot$i.lock"
		if flock -n 9; then
			printf 'slot%d cpu=%s\n' "$i" "${SLOT_CPU[$i]}" >&2
			run_in_slot "$i" "$@"
		fi
		exec 9>&-
	done
	exit 75
	;;
*) usage ;;
esac
