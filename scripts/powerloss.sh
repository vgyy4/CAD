#!/usr/bin/env bash
# Power-loss test on a real block device (ADR 0007, review M14). Needs root, losetup,
# dmsetup with the dm-flakey target and mkfs.ext4 (the Linux CI runner has them).
# Each round: mount ext4 on a linear device-mapper target, run the writer, switch the
# target to dm-flakey drop_writes at a random moment (the disk silently keeps nothing
# from then on), stop the writer, remount and check the last acknowledged save survived.
set -euo pipefail
ROUNDS="${ROUNDS:-8}"
WORK="$(mktemp -d)"
IMG="$WORK/disk.img"
MNT="$WORK/mnt"
mkdir -p "$MNT"
cargo test -p gusset-store --test powerloss --no-run --locked
BIN="$(cargo test -p gusset-store --test powerloss --no-run --locked --message-format=json 2>/dev/null \
  | grep -o '"executable":"[^"]*powerloss[^"]*"' | head -1 | cut -d'"' -f4)"
truncate -s 256M "$IMG"
LOOP="$(sudo losetup -f --show "$IMG")"
SIZE="$(sudo blockdev --getsz "$LOOP")"
sudo modprobe dm-flakey 2>/dev/null || true
cleanup() {
  sudo umount -l "$MNT" 2>/dev/null || true
  sudo dmsetup remove -f flaky 2>/dev/null || true
  sudo losetup -d "$LOOP" 2>/dev/null || true
}
trap cleanup EXIT
linear() { sudo dmsetup create flaky --table "0 $SIZE linear $LOOP 0"; }
linear
sudo mkfs.ext4 -q /dev/mapper/flaky
for round in $(seq 1 "$ROUNDS"); do
  sudo mount /dev/mapper/flaky "$MNT"
  sudo chmod 777 "$MNT"
  GUSSET_POWERLOSS_DIR="$MNT" "$BIN" --ignored --exact write --nocapture > "$WORK/acks" 2>/dev/null &
  WRITER=$!
  sleep "$(awk -v r="$RANDOM" 'BEGIN { print 0.5 + (r % 2500) / 1000 }')"
  # Saves acknowledged before the cut must survive it.
  ACKED="$(grep -E '^[0-9]+$' "$WORK/acks" | tail -1 || echo 0)"
  sudo dmsetup suspend flaky
  sudo dmsetup load flaky --table "0 $SIZE flakey $LOOP 0 0 180 1 drop_writes"
  sudo dmsetup resume flaky
  sleep 0.5
  kill "$WRITER" 2>/dev/null || true
  wait "$WRITER" 2>/dev/null || true
  sudo umount -l "$MNT"
  sudo dmsetup remove flaky
  linear
  sudo mount /dev/mapper/flaky "$MNT"
  echo "round $round: last acknowledged save before the cut: ${ACKED:-0}"
  GUSSET_POWERLOSS_DIR="$MNT" GUSSET_POWERLOSS_MIN="${ACKED:-0}" "$BIN" --ignored --exact check --nocapture
  sudo umount "$MNT"
done
echo "power-loss test passed ($ROUNDS rounds)"
