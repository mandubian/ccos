#!/usr/bin/env python3
"""
migrate_chats_to_repo.py

Create an export of the `chats/` directory into a standalone folder (e.g. `ccos-chats`).
Behavior (safe defaults):
 - Copies all `chats/*.md` as-is.
 - Compresses each `chats/*.json` into `{name}.json.zst` (preferred) or `{name}.json.gz`.
 - Writes a small `{name}.meta.json` for each chat with original size, compressed size and sha256.
 - Produces a summary of total bytes and savings.

The script does NOT push any git remotes. It will not remove files from the original repo unless you pass --remove-originals (dangerous).

Run when you have a good network if you want the script to pull LFS objects (use --fetch-lfs). By default the script will NOT run `git lfs pull` automatically.

Usage examples:
  # dry-run to see what would happen
  python3 scripts/migrate_chats_to_repo.py --output-dir ../ccos-chats --dry-run

  # do the migration, fetch LFS objects first, initialize a git repo in the output
  python3 scripts/migrate_chats_to_repo.py --output-dir ../ccos-chats --fetch-lfs --init-git

"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import stat
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

DEFAULT_OUTPUT = Path(__file__).resolve().parents[1] / "../ccos-chats"


def run(cmd, check=True, capture=False):
    if capture:
        return subprocess.run(cmd, shell=True, check=check, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    return subprocess.run(cmd, shell=True, check=check)


def detect_zstd_binary() -> bool:
    try:
        subprocess.run(["zstd", "--version"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return True
    except FileNotFoundError:
        return False


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def compress_with_zstd_binary(src: Path, dst: Path) -> None:
    # Use -19 (max) and multi-thread (-T0) for best ratio/speed
    cmd = ["zstd", "-19", "-T0", str(src), "-o", str(dst)]
    subprocess.run(cmd, check=True)


def compress_with_gzip(src: Path, dst: Path) -> None:
    import gzip

    with src.open("rb") as f_in, gzip.open(dst, "wb", compresslevel=9) as f_out:
        shutil.copyfileobj(f_in, f_out)


def write_meta(out_dir: Path, basename: str, original_size: int, compressed_size: int, sha256: str, compression: str) -> None:
    meta = {
        "file": basename,
        "original_size": original_size,
        "compressed_size": compressed_size,
        "sha256": sha256,
        "compression": compression,
        "created_at": time.time(),
    }
    meta_path = out_dir / f"{basename}.meta.json"
    with meta_path.open("w", encoding="utf-8") as f:
        json.dump(meta, f, indent=2)


def ensure_output_dir(path: Path, force: bool):
    if path.exists():
        if not path.is_dir():
            raise SystemExit(f"Output path {path} exists and is not a directory")
        if any(path.iterdir()) and not force:
            raise SystemExit(f"Output directory {path} is not empty. Use --force to overwrite or --output-dir to choose another location.")
    else:
        path.mkdir(parents=True, exist_ok=True)


def copy_md_files(src_dir: Path, out_dir: Path, dry_run: bool):
    md_files = sorted(src_dir.glob("*.md"))
    copied = 0
    for f in md_files:
        dest = out_dir / f.name
        if dry_run:
            print(f"[DRY] Would copy MD: {f} -> {dest}")
            copied += 1
            continue
        shutil.copy2(f, dest)
        copied += 1
    return copied


def process_json_file(f: Path, out_dir: Path, use_zstd: bool, dry_run: bool) -> tuple[int, int]:
    basename = f.name
    if use_zstd:
        out_name = f"{basename}.zst"
        compression = "zstd"
    else:
        out_name = f"{basename}.gz"
        compression = "gzip"
    out_path = out_dir / out_name

    original_size = f.stat().st_size

    if dry_run:
        print(f"[DRY] Would compress {f} -> {out_path} using {compression}")
        return original_size, 0

    # Perform compression
    if use_zstd:
        compress_with_zstd_binary(f, out_path)
    else:
        compress_with_gzip(f, out_path)

    compressed_size = out_path.stat().st_size
    sha = sha256_file(out_path)
    write_meta(out_dir, basename, original_size, compressed_size, sha, compression)
    return original_size, compressed_size


def main(argv=None):
    parser = argparse.ArgumentParser(description="Export chats/ into a standalone folder and compress JSONs.")
    parser.add_argument("--output-dir", type=Path, default=Path("../ccos-chats"), help="Directory to create the new repo in (default ../ccos-chats)")
    parser.add_argument("--fetch-lfs", action="store_true", help="Run `git lfs pull --include=\"chats/*\"` before exporting (requires network)")
    parser.add_argument("--init-git", action="store_true", help="Run git init and make an initial commit in the output directory")
    parser.add_argument("--use-zstd", action="store_true", help="Force use zstd binary; if not present the script will error unless --use-gzip is set")
    parser.add_argument("--use-gzip", action="store_true", help="Force use gzip compression instead of zstd")
    parser.add_argument("--dry-run", action="store_true", help="Show what would be done without writing files")
    parser.add_argument("--force", action="store_true", help="Allow writing into a non-empty output directory")
    parser.add_argument("--remove-originals", action="store_true", help="(DANGEROUS) Remove the original chats/*.json from the current repo after copying (NOT recommended)")
    parser.add_argument("--workers", type=int, default=4, help="Parallel workers for compression")

    args = parser.parse_args(argv)

    repo_root = Path.cwd()
    if not (repo_root / ".git").exists():
        print("Warning: current working directory does not look like a git repo root (no .git). Proceeding anyway.")

    src_chats = repo_root / "chats"
    if not src_chats.exists():
        raise SystemExit("chats/ directory not found in current working directory")

    out_dir = (args.output_dir if args.output_dir is not None else DEFAULT_OUTPUT).resolve()

    print(f"Output directory: {out_dir}")
    ensure_output_dir(out_dir, args.force)

    # Optionally fetch LFS objects
    if args.fetch_lfs:
        print("Fetching LFS objects for chats/ (this may use network bandwidth)...")
        try:
            run("git lfs pull --include=\"chats/*\"")
        except Exception as e:
            raise SystemExit(f"git lfs pull failed: {e}")

    # Decide compression method
    zstd_available = detect_zstd_binary()
    if args.use_gzip:
        use_zstd = False
    elif args.use_zstd:
        if not zstd_available:
            raise SystemExit("Requested zstd but 'zstd' binary was not found on PATH")
        use_zstd = True
    else:
        # Default: use zstd if available, else gzip
        use_zstd = zstd_available

    if use_zstd:
        print("Using zstd binary for compression")
    else:
        print("Using gzip compression (built-in) - install zstd for better ratios if desired")

    # Copy MD files
    copied_md = copy_md_files(src_chats, out_dir, args.dry_run)
    print(f"MD files copied: {copied_md}")

    # Find JSON files and compress in parallel
    json_files = sorted(src_chats.glob("*.json"))
    total_original = 0
    total_compressed = 0

    if args.dry_run:
        print(f"[DRY-RUN] Would process {len(json_files)} JSON files")
    else:
        print(f"Processing {len(json_files)} JSON files with {args.workers} workers...")

    futures = []
    results = []
    with ThreadPoolExecutor(max_workers=args.workers) as ex:
        for f in json_files:
            futures.append(ex.submit(process_json_file, f, out_dir, use_zstd, args.dry_run))
        for fut in as_completed(futures):
            try:
                orig, comp = fut.result()
                total_original += orig
                total_compressed += comp
            except Exception as e:
                print(f"Error processing a file: {e}")

    # Summary
    print("\nMigration summary:")
    print(f"  JSON files processed: {len(json_files)}")
    print(f"  Total original bytes: {total_original}")
    print(f"  Total compressed bytes: {total_compressed}")
    if total_original:
        saved = total_original - total_compressed
        pct = saved / total_original * 100
        print(f"  Saved: {saved} bytes ({pct:.1f}%)")

    # Optionally init git
    if args.init_git and not args.dry_run:
        print("Initializing git repo in output directory...")
        try:
            run(f"git -C {out_dir} init")
            run(f"git -C {out_dir} add .")
            run(f"git -C {out_dir} commit -m \"Import compressed chats from main repo\"")
            print("Git repo initialized and initial commit created. Review before pushing to remote.")
        except Exception as e:
            print(f"Git init / commit failed: {e}")

    # Dangerous: remove originals
    if args.remove_originals:
        if args.dry_run:
            print("[DRY] Would remove original JSON files from chats/")
        else:
            print("Removing original JSON files from chats/ (this will delete them from your working tree)")
            for f in json_files:
                try:
                    f.unlink()
                except Exception as e:
                    print(f"Failed to remove {f}: {e}")

    print("Done. Review the output folder before pushing anywhere.")


if __name__ == "__main__":
    main()
