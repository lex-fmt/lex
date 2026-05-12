#!/usr/bin/env python3
"""
Generate the deterministic Lex corpus consumed by `benches/include_resolve.rs`.

Each scenario lives in a self-contained subdirectory (a loader root). The
content is plain prose so per-byte parser cost is predictable; a separate
sanity check during the rig's initial run confirmed the wire codec
round-trips structurally richer documents (sessions, subsections) too.
"""
from pathlib import Path

ROOT = Path(__file__).resolve().parent

LINE = "Lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor.\n"
PARA_LINES = 5
BLANK = "\n"


def paragraphs_for_bytes(target_bytes: int) -> str:
    out = []
    size = 0
    while size < target_bytes:
        for _ in range(PARA_LINES):
            out.append(LINE)
            size += len(LINE)
            if size >= target_bytes:
                break
        out.append(BLANK)
        size += len(BLANK)
    return "".join(out)


def write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)


def include(src: str) -> str:
    return f'\n:: lex.include src="{src}" ::\n\n'


def s1():
    write(ROOT / "s1_no_includes/host.lex", paragraphs_for_bytes(10_000))


def s2():
    d = ROOT / "s2_one_small"
    write(d / "frag.lex", paragraphs_for_bytes(100))
    write(d / "host.lex", paragraphs_for_bytes(2_000) + include("frag.lex") + paragraphs_for_bytes(2_000))


def s3():
    d = ROOT / "s3_one_medium"
    write(d / "frag.lex", paragraphs_for_bytes(10_000))
    write(d / "host.lex", paragraphs_for_bytes(2_000) + include("frag.lex") + paragraphs_for_bytes(2_000))


def s4():
    d = ROOT / "s4_one_large"
    write(d / "frag.lex", paragraphs_for_bytes(100_000))
    write(d / "host.lex", paragraphs_for_bytes(2_000) + include("frag.lex") + paragraphs_for_bytes(2_000))


def s5():
    d = ROOT / "s5_many_small"
    for i in range(50):
        write(d / f"frag{i:02d}.lex", paragraphs_for_bytes(100))
    parts = [paragraphs_for_bytes(500)]
    for i in range(50):
        parts.append(include(f"frag{i:02d}.lex"))
    write(d / "host.lex", "".join(parts))


def s6():
    d = ROOT / "s6_deep_chain"
    write(d / "level5.lex", paragraphs_for_bytes(1_000))
    for i in range(4, 0, -1):
        write(d / f"level{i}.lex", paragraphs_for_bytes(1_000) + include(f"level{i+1}.lex"))
    write(d / "host.lex", paragraphs_for_bytes(500) + include("level1.lex"))


def s7():
    d = ROOT / "s7_realistic"
    write(d / "intro.lex", paragraphs_for_bytes(3_000))
    write(d / "body.lex", paragraphs_for_bytes(15_000))
    write(d / "appendix.lex", paragraphs_for_bytes(2_000))
    write(
        d / "host.lex",
        paragraphs_for_bytes(1_500)
        + include("intro.lex")
        + paragraphs_for_bytes(500)
        + include("body.lex")
        + paragraphs_for_bytes(500)
        + include("appendix.lex"),
    )


def p1_scaling():
    d = ROOT / "p1_10k"
    content = paragraphs_for_bytes(10_000)
    write(d / "host.lex", content)
    write(d / "host.md", content)


def p2_scaling():
    d = ROOT / "p2_100k"
    content = paragraphs_for_bytes(100_000)
    write(d / "host.lex", content)
    write(d / "host.md", content)


def p3_scaling():
    d = ROOT / "p3_1m"
    content = paragraphs_for_bytes(1_000_000)
    write(d / "host.lex", content)
    write(d / "host.md", content)


def main():
    for fn in (s1, s2, s3, s4, s5, s6, s7, p1_scaling, p2_scaling, p3_scaling):
        fn()
    print("scenario,host_bytes,total_bytes")
    for sub in sorted(ROOT.iterdir()):
        if not sub.is_dir():
            continue
        host = sub / "host.lex"
        if not host.exists():
            continue
        total = sum(p.stat().st_size for p in sub.glob("*.lex"))
        print(f"{sub.name},{host.stat().st_size},{total}")


if __name__ == "__main__":
    main()
