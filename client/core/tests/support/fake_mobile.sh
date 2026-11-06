#!/bin/sh
# Keep executable bytes immutable while parallel tests fork. Each scratch
# program publishes its complete cases as non-executable input beside its link.
exec /bin/sh "$0.fixture" "$@"
