"""Build the no_std Rust library only for the opt-in PlatformIO environment."""
Import("env")
from pathlib import Path
import subprocess

root = Path(env.subst("$PROJECT_DIR"))
crate = root / "rust" / "audio-core"
target = "xtensa-esp32s3-none-elf"
subprocess.run([
    "rustup", "run", "esp", "cargo", "build", "--release",
    "--target", target, "-Zbuild-std=core",
    "--manifest-path", str(crate / "Cargo.toml"),
], check=True, cwd=root)
env.Append(LIBPATH=[str(crate / "target" / target / "release")])
env.Append(LIBS=["stackchan_audio_core"])
