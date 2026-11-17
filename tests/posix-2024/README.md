# POSIX.1-2024 coverage

DOS-RS runs native POSIX user programs on i686 BIOS and x86_64 UEFI. The 16-bit 286/386 kernel has its own DOS and Windows regression suite. A result for an ELF target applies only to that target.

The inventory covers the three normative volumes of Issue 8. Each named header, function and utility has a source locator. Chapter headings have separate locators, including the shell grammar and the special built-ins embedded in chapter 2. Resolve a source by appending its locator to the [Issue 8 publication](https://pubs.opengroup.org/onlinepubs/9799919799/).

| File | Contents |
| --- | --- |
| [units.tsv](units.tsv) | 2,325 named entries and chapter headings, including individual names on shared manual pages. Each row carries its applicability and both architecture results. |
| [evidence.tsv](evidence.tsv) | Individual tested behaviors, implementation choices and reproducible test-case IDs. The sparse file is the maintained input to the generator. |
| Generated `obligations.tsv` | Clause locators and occurrence numbers for normative `shall` wording and `implementation-defined` choices. IDs refer to occurrences within each heading. |

`base` in a signature means the named interface is available in the base profile. Margin codes such as `XSI`, `SPN` and `FSC|SIO` retain their Issue 8 spelling. `CX` marks a POSIX extension to ISO C, and `OB` marks obsolescence. Individual paragraphs can have additional conditions. Conditions shared by only some overloads or imposed in prose require review against the linked text. DOS-RS currently declares no optional POSIX option group as fully implemented. [Issue 8 margin codes](https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/V1_chap01.html#tag_01_08_01)

`open` means the entry lacks recorded acceptance. `partial` identifies specific exercised cases. `passed` on a clause records the listed behavior. A formal conformance review still needs to account for normative rules expressed without either indexed wording, subjects imposing obligations on applications, and all subclauses applicable to the implementation. Coreutils differential results cover the commands and inputs recorded in their JSON reports. The initial report covers seven Rust tools in the C and C.UTF-8 locales.

## Regenerate

Use a local Issue 8 tree with its official `basedefs`, `functions` and `utilities` directories and `contents.html` indices. The generator also accepts `--basedefs`, `--functions` and `--utilities` for separately saved indices.

```sh
python3 scripts/posix-issue8-inventory.py --spec "$ISSUE8" --output target/posix-issue8
diff -u tests/posix-2024/units.tsv target/posix-issue8/units.tsv
```

## Reproduce utility evidence

Build the native musl sysroots or set `DOSRS_MUSL_ARCHIVE` to the musl 1.2.6 release archive. The Rust tools link to that DOS-RS libc. The runner requires unprefixed GNU coreutils 9.12 binaries, QEMU, mtools, the cross compilers and `fsck.fat`. `FSCK_FAT` can name the installed checker. Supply a RAM volume large enough for both disposable guest disks.

```sh
python3 scripts/qemu-coreutils.py --ram-dir "$RAM_DIR" --gnu-bin "$GNU_BIN" --proof target/coreutils-proof
```

The runner forks a disk-loaded ELF for each case, redirects its descriptors, records its wait status and shuts down QEMU. The host reads the stopped FAT disk and compares stdout, stderr, exit status and input file contents against the identified GNU build. The per-architecture JSON saves fixture, guest ELF, oracle ELF and output hashes. The kernel trace records each executed path.
