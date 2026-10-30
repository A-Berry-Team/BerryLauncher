#!/usr/bin/env python3
"""Fail when the project version differs between meson.build, Cargo.toml and the metainfo.

Usage: check-version.py <meson-version> <Cargo.toml> <metainfo.xml.in>
"""
import re
import sys
import xml.etree.ElementTree as ET


def cargo_version(path, section="[package]"):
    in_package = False
    for line in open(path, encoding="utf-8"):
        stripped = line.strip()
        if stripped.startswith("["):
            in_package = stripped == section
        elif in_package:
            match = re.match(r'version\s*=\s*"([^"]+)"', stripped)
            if match:
                return match.group(1)
    sys.exit(f"no {section} version in {path}")


def metainfo_version(path):
    release = ET.parse(path).getroot().find("./releases/release")
    if release is None:
        sys.exit(f"{path} has no <releases><release/></releases>")
    return release.get("version")


def main():
    meson, cargo_toml, metainfo = sys.argv[1:4]
    found = {"meson.build": meson, "Cargo.toml": cargo_version(cargo_toml), "workspace.package": cargo_version(cargo_toml, "[workspace.package]"), "metainfo": metainfo_version(metainfo)}
    if len(set(found.values())) != 1:
        sys.exit("version mismatch: " + ", ".join(f"{k}={v}" for k, v in found.items()))
    print("version", meson)


if __name__ == "__main__":
    main()
