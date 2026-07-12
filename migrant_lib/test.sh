#!/bin/bash
# Run the migrant_lib test suite against throwaway docker databases.
#
# Sqlite tests are self-contained (bundled, temp files / in-memory).
# Postgres and mysql tests are gated on POSTGRES_TEST_CONN_STR /
# MYSQL_TEST_CONN_STR; this script provides both via docker.
set -e

PG_CONTAINER=migrant-test-pg
MYSQL_CONTAINER=migrant-test-mysql
PG_PORT="${PG_PORT:-15432}"
MYSQL_PORT="${MYSQL_PORT:-13306}"

cleanup() {
    docker stop "$PG_CONTAINER" >/dev/null 2>&1 || true
    docker stop "$MYSQL_CONTAINER" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker run -d --rm --name "$PG_CONTAINER" \
    -e POSTGRES_USER=migrant \
    -e POSTGRES_PASSWORD=pass \
    -e POSTGRES_DB=migrant_test \
    -p "$PG_PORT:5432" \
    postgres:16-alpine >/dev/null

docker run -d --rm --name "$MYSQL_CONTAINER" \
    -e MYSQL_ROOT_PASSWORD=rootpass \
    -e MYSQL_DATABASE=migrant_test \
    -e MYSQL_USER=migrant \
    -e MYSQL_PASSWORD=pass \
    -p "$MYSQL_PORT:3306" \
    mysql:8 >/dev/null

echo "waiting for postgres..."
until docker exec "$PG_CONTAINER" pg_isready -U migrant >/dev/null 2>&1; do sleep 1; done
echo "waiting for mysql..."
until docker exec "$MYSQL_CONTAINER" mysqladmin ping -prootpass >/dev/null 2>&1; do sleep 1; done

set -x
cargo test
POSTGRES_TEST_CONN_STR="postgres://migrant:pass@localhost:$PG_PORT/migrant_test" \
MYSQL_TEST_CONN_STR="mysql://migrant:pass@localhost:$MYSQL_PORT/migrant_test" \
    cargo test --features d-all -- --nocapture
set +x

echo ""
echo "** Tests complete! **"
