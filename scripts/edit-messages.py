"""Generate the native Edit message table and typed keys from UTF-8 sources."""

import json
from pathlib import Path
import sys

catalog = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
out = Path(sys.argv[2])
if list(catalog) != ["de", "en", "es", "fr", "ja", "ko", "zh-Hans", "zh-Hant"]:
    raise ValueError("Edit and CONFIG.SYS must use the same language tags")
keys = list(catalog["en"])
arms = []
for tag, items in catalog.items():
    if set(items) != set(keys):
        raise ValueError(f"incomplete messages for {tag}")
    for key in keys:
        value = items[key]
        if not value.strip() or any(ord(c) < 32 for c in value):
            raise ValueError(f"invalid message {tag}/{key}")
        columns = sum(1 if ord(c) < 128 else 2 for c in value)
        if columns > (73 if key == "bar" else 76):
            raise ValueError(f"{tag}/{key} would wrap at 80 columns")
        variant = key.title().replace("_", "")
        encoded = "".join(f"\\x{byte:02x}" for byte in value.encode("gb18030"))
        arms.append(
            f'({list(catalog).index(tag)}, Message::{variant}) => b"{encoded}",'
        )
variants = ", ".join(k.title().replace("_", "") for k in keys)
(out / "edit-messages.rs").write_text(
    f"#[repr(u16)] #[derive(Clone, Copy)] pub enum Message {{ {variants} }}\n"
    "fn translated(language: u16, message: Message) -> &'static [u8] {\n"
    "match (language, message) {\n" + "\n".join(arms) + '\n_ => b"",\n}\n}\n',
    encoding="utf-8",
)
