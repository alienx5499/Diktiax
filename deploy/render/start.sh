#!/bin/sh
# FalkorDB on loopback only (not reachable from outside the container), then the backend.
set -e
redis-server --bind 127.0.0.1 --port 6379 --save "" --appendonly no --dir /tmp --pidfile /tmp/redis.pid \
  --loadmodule /var/lib/falkordb/bin/falkordb.so --daemonize yes
i=0
until redis-cli -h 127.0.0.1 ping >/dev/null 2>&1; do
  i=$((i + 1)); [ "$i" -gt 50 ] && echo "FalkorDB did not start" >&2 && exit 1
  sleep 0.2
done
exec diktiax-backend
