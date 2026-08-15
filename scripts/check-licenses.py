#!/usr/bin/env python3
"""Fail if published crates lose their dual license, or unpublished ones claim it."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PUBLISHED = {
    "crates/cortex-domain",
    "crates/cortex-context",
    "crates/cortex-router",
    "crates/cortex-skills",
}


def manifest_license(path: Path) -> str | None:
    text = path.read_text(encoding="utf-8")
    for line in text.splitlines():
        if line.startswith("license"):
            return line.split("=", 1)[1].strip().strip('"')
    return None


def main() -> int:
    errors: list[str] = []
    for cargo in ROOT.glob("**/Cargo.toml"):
        rel = cargo.parent.relative_to(ROOT).as_posix()
        if rel == "." or "target" in cargo.parts:
            continue
        license_field = manifest_license(cargo)
        published = rel in PUBLISHED
        if published:
            if license_field != "MIT OR Apache-2.0":
                errors.append(f"{rel}: published crate must be MIT OR Apache-2.0")
            for name in ("LICENSE-MIT", "LICENSE-APACHE"):
                if not (cargo.parent / name).is_file():
                    errors.append(f"{rel}: missing {name}")
        elif license_field == "MIT OR Apache-2.0":
            errors.append(f"{rel}: unpublished crate claims MIT OR Apache-2.0")
    if errors:
        print("license policy failed:")
        for error in errors:
            print(f"  {error}")
        return 1
    print("license policy ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
