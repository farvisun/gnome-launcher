#!/usr/bin/env bash
# Source this only when using the optional local headers from bootstrap-build.py.
task_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
if [[ -d "$task_root/.build-deps/root/usr/lib/x86_64-linux-gnu/pkgconfig" ]]; then
  export PKG_CONFIG_PATH="$task_root/.build-deps/root/usr/lib/x86_64-linux-gnu/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
fi
unset task_root
