#!/usr/bin/env bash
# build-oracle.sh [SMASH]: builds the oracle and writes the reference records to stdout
set -e
OCCT=/tmp/claude-0/-home-user-CAD/cf7e9ad3-f27d-536b-96de-7672b2bfebc7/scratchpad/deps/occt-install
mkdir -p oracle-bin
c++ -std=c++17 -O2 oracle/mathpoly.cpp -I$OCCT/include/opencascade -L$OCCT/lib -lTKMath -lTKernel -o oracle-bin/mathpoly
export LD_LIBRARY_PATH=$OCCT/lib SMASH=${1:-0}
oracle-bin/mathpoly
