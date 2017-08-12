#!/bin/bash


PROJ_DIR=`dirname $0`
mkdir -p "$PROJ_DIR/db"
SQLITE=$PROJ_DIR/db/_test_db.db

touch "$SQLITE"

SQLITE_TEST_CONN_STR=$SQLITE cargo test

rm "$SQLITE"
touch "$SQLITE"

SQLITE_TEST_CONN_STR=$SQLITE cargo test --features sqlite

rm "$SQLITE"
