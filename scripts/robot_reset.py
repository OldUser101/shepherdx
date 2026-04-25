#!/usr/bin/env python3

import sys

try:
    import robot.reset
    robot.reset.reset()
except ImportError:
    print("failed to locate robot library!")
    sys.exit(1)
