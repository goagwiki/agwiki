"""Smoke test for the `agwiki` extension module.

Builds a minimal wiki in a temp dir and exercises `check_wiki`, asserting it
returns a list. Run after `maturin develop` (or with the built wheel installed).
"""

import os
import tempfile

import agwiki


def main() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        wiki_dir = os.path.join(tmp, "wiki")
        os.makedirs(wiki_dir, exist_ok=True)
        with open(os.path.join(wiki_dir, "index.md"), "w", encoding="utf-8") as f:
            f.write("# Index\n\nWelcome to the smoke-test wiki.\n")

        problems = agwiki.check_wiki(tmp)
        assert isinstance(problems, list), f"expected list, got {type(problems)!r}"
        for p in problems:
            assert isinstance(p, dict), f"expected dict problem, got {type(p)!r}"
            assert "kind" in p and "message" in p, f"missing keys in {p!r}"

        print(f"check_wiki OK: {len(problems)} problem(s) -> {problems}")
        print("smoke test PASSED")


if __name__ == "__main__":
    main()
