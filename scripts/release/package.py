"""Package allowlisted files built from an immutable release commit."""
import argparse
import hashlib
import json
import shutil
import subprocess
import tarfile
import tomllib
import zipfile
from pathlib import Path


def version(command):
    return subprocess.check_output(command, text=True).strip()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("kind", choices=["native", "wasm"])
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--target")
    args = parser.parse_args()
    source = args.source.resolve()
    assert version(["git", "-C", str(source), "rev-parse", "HEAD"]) == args.commit
    assert tomllib.loads((source / "Cargo.toml").read_text())["workspace"]["package"]["version"] == args.tag[1:]
    args.output.mkdir(parents=True, exist_ok=True)
    names = ["native"] if args.kind == "native" else ["replay", "recommend"]
    for module in names:
        name = (f"ournotes-deck-{args.tag}-{args.target}" if module == "native"
                else f"ournotes-{module}-wasm-{args.tag}")
        staging = source / "work" / "release-packages" / name
        staging.mkdir(parents=True, exist_ok=False)
        for item in ["LICENSE-MIT", "LICENSE-APACHE", "README.md", "README.en.md", "CHANGELOG.md"]:
            shutil.copy2(source / item, staging / item)
        metadata = {"version": args.tag[1:], "tag": args.tag, "commit": args.commit,
                    "rustc": version(["rustc", "--version"]), "kind": args.kind}
        if module == "native":
            assert args.target
            metadata["target"] = args.target
            extension = ".exe" if "windows" in args.target else ""
            for binary in ["ournotes-deck", "ournotes-recommend"]:
                shutil.copy2(source / "target" / args.target / "release" / (binary + extension), staging)
            usage = "Run ./ournotes-deck power --data deck-data.json --roster box.json -k 10.\n"
            usage += "Run ./ournotes-recommend --help for the JSON recommendation CLI.\n"
            if extension:
                usage += "On Windows use .\\ournotes-deck.exe and .\\ournotes-recommend.exe.\n"
            if "linux" in args.target:
                usage += "Linux binaries use musl and are statically linked.\n"
            if "apple" in args.target:
                usage += "Requires macOS 11 or later.\n"
        else:
            metadata.update(module=module, target="wasm32-unknown-unknown", wasmBindgen=version(["wasm-bindgen", "--version"]))
            for binding in ["web", "nodejs"]:
                folder = staging / binding
                shutil.copytree(source / "work" / "bindings" / module / binding, folder)
                package = {"name": f"ournotes-{module}-wasm-{binding}", "version": args.tag[1:],
                           "private": True, "type": "module" if binding == "web" else "commonjs"}
                (folder / "package.json").write_text(json.dumps(package, indent=2) + "\n")
            usage = f"Web: import init, {{ {'DeckSolver' if module == 'recommend' else 'ReplaySession'} }} from './web/ournotes_{module}_wasm.js'; await init();\n"
            usage += f"Node: const {{ {'DeckSolver' if module == 'recommend' else 'ReplaySession'} }} = require('./nodejs/ournotes_{module}_wasm.js');\n"
            usage += "Keep each JS file beside its matching _bg.wasm file. TypeScript declarations are included.\n"
            usage += "Serve web files over HTTP with application/wasm for .wasm. Recommendation runs synchronously; use a dedicated Worker.\n"
            if module == "recommend":
                shutil.copy2(source / "wasm" / "recommend" / "README.md", staging / "ADAPTER.md")
        usage += "Provide your own matching-version deck data; no game data is bundled.\n"
        (staging / "ARTIFACT-README.txt").write_text(usage, encoding="utf-8")
        metadata["files"] = {str(p.relative_to(staging)).replace("\\", "/"): hashlib.sha256(p.read_bytes()).hexdigest()
                             for p in sorted(staging.rglob("*")) if p.is_file()}
        (staging / "build-info.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
        if module == "native" and "windows" in args.target:
            archive = args.output / (name + ".zip")
            with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as output:
                for p in sorted(staging.rglob("*")):
                    if p.is_file():
                        output.write(p, str(p.relative_to(staging.parent)))
        else:
            archive = args.output / (name + ".tar.gz")
            with tarfile.open(archive, "w:gz") as output:
                output.add(staging, arcname=name)
        print(f"{archive.name}: {archive.stat().st_size} bytes, sha256 {hashlib.sha256(archive.read_bytes()).hexdigest()}")


if __name__ == "__main__":
    main()
