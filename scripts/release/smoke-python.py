"""Install a built ournotes-sim wheel in a fresh environment and run the package tests against it."""
import argparse
import os
import subprocess
import tempfile
import venv
from pathlib import Path

CHECK = """
import sys
from pathlib import Path

import ournotes_sim

assert Path(ournotes_sim.__file__).resolve().is_relative_to(Path(sys.prefix).resolve()), ournotes_sim.__file__
print(ournotes_sim.info()["commit"])
"""


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("dist", type=Path, help="directory holding exactly one wheel")
    parser.add_argument("--tests", required=True, type=Path, help="the package tests (python/tests)")
    parser.add_argument("--commit", help="the source commit the wheel must report")
    args = parser.parse_args()
    wheel, = args.dist.resolve().glob("*.whl")
    tests = args.tests.resolve()
    with tempfile.TemporaryDirectory(prefix="ournotes-sim-wheel-") as directory:
        root = Path(directory)
        venv.EnvBuilder(with_pip=True).create(root / "venv")
        python = root / "venv" / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
        subprocess.run([python, "-I", "-m", "pip", "install", "--quiet", str(wheel), "pytest>=8"], cwd=root, check=True)
        subprocess.run([python, "-I", "-m", "pytest", "-q", "-p", "no:cacheprovider", str(tests)], cwd=root, check=True)
        commit = subprocess.check_output([python, "-I", "-c", CHECK], cwd=root, encoding="utf-8").strip()
        if args.commit:
            assert commit == args.commit, f"the wheel reports commit {commit}, not {args.commit}"
        print(f"{wheel.name}: tests passed, commit {commit}")


if __name__ == "__main__":
    main()
