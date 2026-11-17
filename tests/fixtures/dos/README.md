# DOS compatibility fixtures

These DOS programs exercise the CPU, DOS ABI, and driver packet format of a fixed historical client. The original sources and build-time defines are recorded below. Run new application keyboard tests through the emulator. [Original revision](https://github.com/JamesLinYJ/DOS-RS/tree/b4aea4d3ad477eebebe8b70e63345973af1f8844/tests)

| Client | Original source | Defines | SHA-256 |
| --- | --- | --- | --- |
| CHECKMEM.COM | extended-memory.asm | — | `3dc3637029aff37f001899ee6e44eb4f8687cceda9534dc7205bdaa74611409d` |
| FILES.COM | native16/files.asm | — | `056aee55d6f6dc2bd6ec2bab85c11da18b91d1e98ca1cb0fa1259ff5df1f19ab` |
| SPIN.COM | terminals.asm | SPIN | `5dd9bc8fafbdd1d8abbb7f5608fa02ab36f828e234011677a1c4725cb5da68cf` |
| WAITKEY.COM | terminals.asm | WAITKEY | `fce444bb6fbc6f84c00cb410dacaa8db82a85c9cf9f29fc7a53636c6a504561c` |
| CRASH.COM | terminals.asm | CRASH | `7df47c715183a35bc3edb5ec5e71a297f416a9c8f861413b2beb4906bee8190c` |
| CALC.COM | terminals.asm | CALC | `aeaba6bd1b45ed1d8688d90efede96296b1f81d4a5ca4a10041cfeddaf06e48b` |
| REJECT.SYS | devices.asm | CHARACTER, REJECT | `ac401a00c80c44ba96cff4eb6859d29b5d7a6cd8d47648c902a027db0a1d4fed` |
| ECHO.SYS | devices.asm | CHARACTER, CHAIN | `0c69617d3d3e579ea1fa382d165701d82a43ee43df86f7712ad01470d23f854f` |
| BLOCK.SYS | devices.asm | BLOCK | `f17365457fd47b99e895d2e1f9ff62b39ab75795367ea035438608a4161b839f` |
| PROBE.COM | devices.asm | — | `5a3a0062bf28c2b2f17a654e7608e82a2327b12edd2c2581a27ea9b98adef4a1` |
| PROBEVER.COM | devices.asm | SETVER | `54fa665b99c1800ac44478109e2e71258815bc3a7b482a17c549c580d374907d` |
| architecture.com | architecture.asm | — | `c65de0c0c53c376f5af4a19e083c61ff92145a69a43f0224b1d738646c380eac` |

ECHO.SYS includes the original 32-byte DOS MZ device header. PROBEVER.COM is loaded as PROBE.COM when testing SETVER. The architecture client remains at `tests/architecture.com`. These fixtures are excluded from the installation media.

[PSPPLACE](pspplace.c) is built as a 386 real-mode COM with Smaller C. It checks the process and environment MCB owners, reads back its file and reports the PSP segment on standard output. Build the host Smaller C tools, then pass their checkout via `--smaller-c` to [the native EXEC placement test](../../../scripts/realmode-exec-placement.py), together with `--toolchain`, `--image`, `--ram-dir`, `--proof` and `--dosbox`. The 286 run uses a Rust16 MZ client and checks the granted paragraphs against its executable header. [Smaller C revision](https://github.com/alexfru/SmallerC/tree/1865d79ce7a5ad3f8a9515a571437cee084b8b1d)
