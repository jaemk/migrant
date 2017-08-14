#!/bin/bash
set -x

PROJ_DIR=`dirname $0`
mkdir -p "$PROJ_DIR/db"
SQLITE=$PROJ_DIR/db/_test_db.db
POSTGRES=postgres://__migrant_testing:pass@localhost/__migrant_testing

function setup() {
    touch "$SQLITE"
    sudo -u postgres createuser __migrant_testing
    sudo -u postgres psql -c "alter user __migrant_testing with password 'pass'"
    sudo -u postgres createdb __migrant_testing
}

function teardown() {
    rm "$SQLITE"
    sudo -u postgres dropdb __migrant_testing
    sudo -u postgres psql -c 'drop user __migrant_testing'
}

setup
SQLITE_TEST_CONN_STR=$SQLITE POSTGRES_TEST_CONN_STR=$POSTGRES cargo test -- --nocapture

teardown
setup

SQLITE_TEST_CONN_STR=$SQLITE POSTGRES_TEST_CONN_STR=$POSTGRES cargo test --features 'sqlite postgresql' -- --nocapture

teardown
