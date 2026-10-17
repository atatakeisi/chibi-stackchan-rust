#!/usr/bin/env python3
"""Build/test the Rust application without changing shell environment variables."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
CRATE = ROOT / "rust" / "firmware"
TARGET = "xtensa-esp32s3-espidf"


def create_image():
    """Create an application image only. Never connect to a device."""
    elf = CRATE / "target" / TARGET / "release" / "stackchan-firmware"
    if not elf.is_file():
        raise RuntimeError("Build the firmware before creating its image.")
    candidates = sorted((CRATE / ".embuild" / "espressif" / "python_env").glob("idf5.4_*/bin/python3"))
    for interpreter in candidates:
        check = subprocess.run([str(interpreter), "-m", "esptool", "version"], capture_output=True)
        if check.returncode == 0:
            break
    else:
        raise RuntimeError("ESP-IDF's esptool environment is missing; run the target build first.")
    binary = elf.with_suffix(".bin")
    subprocess.run([str(interpreter), "-m", "esptool", "--chip", "esp32s3",
                    "elf2image", "--flash_mode", "qio", "--flash_freq", "80m",
                    "--flash_size", "8MB", "--output", str(binary), str(elf)], check=True)
    size = binary.stat().st_size
    if size > 0x330000:
        raise RuntimeError(f"Application image exceeds app0 size: {size} > {0x330000} bytes")
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    print(f"Application image: {binary.relative_to(ROOT)} ({size:,} bytes)")
    print(f"SHA-256: {digest}")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["test", "build", "check", "tools", "image"])
    args = parser.parse_args()
    if args.command == "image":
        return create_image()
    if args.command == "tools":
        return subprocess.call(["cargo", "install", "ldproxy", "--root", str(ROOT / ".tools"), "--locked"], cwd=ROOT)
    if args.command == "test":
        result = subprocess.call(["cargo", "test", "--locked", "--offline"], cwd=ROOT / "rust" / "avatar-face")
        if result != 0:
            return result
        return subprocess.call(["cargo", "test", "--locked", "--offline"], cwd=CRATE)
    linker = shutil.which("ldproxy") or str(ROOT / ".tools" / "bin" / "ldproxy")
    if not Path(linker).is_file():
        parser.error("ldproxy is missing. Run: python3 scripts/firmware.py tools")
    # ESP-IDF's CMake project is generated inside target/, so the partition
    # filename must be absolute. Generate this machine-specific defaults file.
    partition_file = CRATE / "partitions.csv"
    partition = str(partition_file).replace("\\", "/")
    # esp-idf-sys tracks defaults files. Include the CSV digest so editing the
    # partition layout also regenerates its CMake partition-table artifact.
    partition_digest = hashlib.sha256(partition_file.read_bytes()).hexdigest()
    defaults = f'# Partition CSV SHA-256: {partition_digest}\n'
    defaults += 'CONFIG_PARTITION_TABLE_CUSTOM_FILENAME=' + json.dumps(partition) + '\n'
    output = CRATE / "sdkconfig.partition.defaults"
    if not output.exists() or output.read_text() != defaults:
        output.write_text(defaults)
    action = "build" if args.command == "build" else "check"
    command = ["rustup", "run", "esp", "cargo", action, "--release", "--locked",
               "--target", TARGET, "-Zbuild-std=std,panic_abort", "--config",
               f"target.{TARGET}.linker={json.dumps(linker)}"]
    result = subprocess.call(command, cwd=CRATE)
    if result == 0 and args.command == "build":
        return create_image()
    return result


if __name__ == "__main__":
    sys.exit(main())
