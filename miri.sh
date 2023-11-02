#!/bin/bash
MIRIFLAGS="" cargo +nightly miri test -- $1
