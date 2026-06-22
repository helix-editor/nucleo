#!/usr/bin/env bash
set -e

UCD_VERSION=16.0.0
dir=$(pwd)
mkdir /tmp/ucd-$UCD_VERSION
cd /tmp/ucd-$UCD_VERSION
curl -LO https://www.unicode.org/Public/zipped/$UCD_VERSION/UCD.zip
unzip UCD.zip

cd "${dir}"
cargo install ucd-generate
ucd-generate case-folding-simple /tmp/ucd-$UCD_VERSION --chars > src/chars/case_fold.rs
rm -rf /tmp/ucd-$UCD_VERSION
