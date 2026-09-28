#!/usr/bin/env python3
"""Compares two tools/pixel-baseline.sh outputs byte for byte.

Usage: pixel-compare.py <base dir> <head dir>

Compares every .png, .rgba and .scenes file; exits 1 if any differs or
exists on one side only. The one exception: an export frame on one side
only that repeats that side's last common frame byte for byte (how many
captures finish before an export exits varies between runs of one build).
"""
import filecmp
import os
import sys

EXTENSIONS = (".png", ".rgba", ".scenes")


def files(root):
    found = set()
    for directory, _, names in os.walk(root):
        for name in names:
            if name.endswith(EXTENSIONS):
                found.add(os.path.relpath(os.path.join(directory, name), root))
    return found


base, head = sys.argv[1], sys.argv[2]
before, after = files(base), files(head)
common = sorted(before & after)
different = [
    path
    for path in common
    if not filecmp.cmp(os.path.join(base, path), os.path.join(head, path), shallow=False)
]
only = sorted(before ^ after)


def repeats_final_frame(path):
    """Whether `path`, an export frame present on one side only, repeats
    that side's last frame in common, byte for byte. Exports stop on a frame
    count, and how many captures finish before the app exits varies between
    runs of the same build; every frame both sides have is still compared.
    """
    directory, name = os.path.split(path)
    if not path.startswith("exports" + os.sep) or not name[:-4].isdigit():
        return False
    root = base if path in before else head
    common_frames = sorted(
        other for other in common if os.path.dirname(other) == directory
    )
    return bool(common_frames) and filecmp.cmp(
        os.path.join(root, path), os.path.join(root, common_frames[-1]), shallow=False
    )


repeated = [path for path in only if repeats_final_frame(path)]
only = [path for path in only if path not in repeated]
for path in different:
    print(f"differs: {path}")
for path in only:
    print(f"only in {'base' if path in before else 'head'}: {path}")
for path in repeated:
    print(f"repeated final frame, only in {'base' if path in before else 'head'}: {path}")
groups = {}
for path in common:
    group = path.split(os.sep)[0]
    groups[group] = groups.get(group, 0) + 1
print("compared:", ", ".join(f"{count} {group}" for group, count in sorted(groups.items())))
print(f"{len(common)} files compared, {len(different)} differ, {len(only)} unmatched")
sys.exit(1 if different or only else 0)
