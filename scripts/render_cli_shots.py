"""Render captured CLI transcripts as terminal screenshots."""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "docs" / "images"
CAPTURE = ROOT / ".cortex-loom" / "cli-shots"


def font(size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    for name in (
        "C:/Windows/Fonts/consola.ttf",
        "C:/Windows/Fonts/CascadiaMono.ttf",
        "C:/Windows/Fonts/lucon.ttf",
    ):
        path = Path(name)
        if path.exists():
            return ImageFont.truetype(str(path), size)
    return ImageFont.load_default()


def render(name: str, title: str, body: str, width: int = 980) -> None:
    lines = body.replace("\r\n", "\n").rstrip("\n").split("\n")
    if len(lines) > 28:
        lines = lines[:28] + ["…"]
    face = font(16)
    pad = 28
    line_h = 22
    height = pad * 2 + 36 + line_h * (len(lines) + 1)
    image = Image.new("RGB", (width, height), "#12141a")
    draw = ImageDraw.Draw(image)
    draw.rectangle((0, 0, width, 36), fill="#1c1f27")
    draw.ellipse((16, 12, 28, 24), fill="#ff5f57")
    draw.ellipse((36, 12, 48, 24), fill="#febc2e")
    draw.ellipse((56, 12, 68, 24), fill="#28c840")
    draw.text((84, 10), title, fill="#d7dbe3", font=face)
    y = 52
    for line in lines:
        draw.text((pad, y), line[:120], fill="#e8edf5", font=face)
        y += line_h
    OUT.mkdir(parents=True, exist_ok=True)
    dest = OUT / name
    image.save(dest)
    print(f"wrote {dest}")


def main() -> None:
    for name, title, filename in (
        ("cli-help.png", "cortex-loom --help", "help.txt"),
        ("cli-doctor.png", "cortex-loom doctor --repo .", "doctor.txt"),
        ("cli-setup.png", "cortex-loom setup --agent claude-code --dry-run", "setup.txt"),
    ):
        text = (CAPTURE / filename).read_text(encoding="utf-8", errors="replace")
        render(name, title, text)


if __name__ == "__main__":
    main()
