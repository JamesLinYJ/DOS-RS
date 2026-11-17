"""Build a native DOS client against the same 286 ABI as the install disk."""
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def build_example(tools, name):
    target = ROOT / "target/native16"
    objects = (target / "util/io.obj", target / "emm/client.obj")
    env = dict(os.environ, RUST16_CPU="286", RUST16_PLATFORM="dos",
               RUST16_OBJECTS=os.pathsep.join(map(str, objects)))
    subprocess.run([tools.resolve() / "scripts/cargo16.sh", "--locked", "--release",
                    "--manifest-path", ROOT / "native-tools/Cargo.toml", "--target-dir",
                    target / "util", "--example", name], check=True, env=env)
    return target / "util/dos16/release/examples" / (name + ".exe")
