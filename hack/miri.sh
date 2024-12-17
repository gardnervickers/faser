#!/bin/bash
MIRIFLAGS="" cargo miri test -- $1
