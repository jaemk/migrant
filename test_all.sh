#!/bin/bash

set -ex

cd migrant_lib
./test.sh
cd ..
cargo test
