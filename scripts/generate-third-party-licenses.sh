#!/usr/bin/env sh
# SPDX-License-Identifier: MIT
# Regenerate the checked-in third-party notice from Cargo.lock.
set -eu

required_version=0.9.3
actual_version="$(cargo about --version | awk '{print $2}')"
test "$actual_version" = "$required_version"

cargo about generate licenses/third-party-notices.hbs > THIRD_PARTY_LICENSES.html
