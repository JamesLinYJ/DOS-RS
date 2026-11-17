# 原生 DOS 的高物理内存

装入 `DEVICE=EMM386.SYS` 后，16 位 DOS 程序可通过 `INT 2Fh` 申请 4 GiB 以上的 RAM 页，将页内数据复制到自己拥有的常规内存。每页 4096 字节，驱动共有 16 个页句柄，每个句柄归分配它的 DOS 进程所有。页句柄由驱动分配，普通段指针和现有 EMS、XMS、VCPI 接口保持各自的寻址方式。

驱动在真实模式下检查 CPUID、PAE 和 PSE，读取 BIOS 的完整 E820 内存表。分配上限为处理器报告的物理地址位数和 48 两者的较小值。驱动只使用完全位于可用 RAM 中的整页，避开固件列出的保留区。80286 启动时跳过这套驱动。缺少 PAE、无法取得完整内存表或没有可用高 RAM 时仍可运行原有 DOS 程序。

## DOS 调用约定

先确认 `INT 2Fh` 已安装。每次调用都传入 `DI=4850h`。驱动识别请求后返回 `DI=5048h`。下表的地址均按十六进制寄存器名所示，从最高字到最低字排列。

| AX | 输入 | 返回 |
| --- | --- | --- |
| D7B0h | 查询 | AX=1 表示有可分配的高 RAM 池。BX 是该接口使用的物理地址位数。AX=0 表示未启用。 |
| D7B1h | DX:CX:BX 为期望的最低物理地址，可为零 | AX 为页句柄，DX:CX:BX 为实际物理字节地址。AX=0 表示分配失败。新分配的整页清零。 |
| D7B2h | DX 为句柄，BX 为页内偏移，CX 为字节数，ES:SI 为目标缓冲区 | 从高 RAM 复制到进程缓冲区。AX=1 成功，AX=0 失败。 |
| D7B3h | DX 为句柄，BX 为页内偏移，CX 为字节数，ES:SI 为源缓冲区 | 从进程缓冲区复制到高 RAM。AX=1 成功，AX=0 失败。 |
| D7B4h | DX 为句柄 | 释放页。AX=1 成功，AX=0 表示无效句柄。 |

复制范围须在同一页内。常规内存缓冲区须在同一个 DOS 分配块内，且不能跨越段结尾。每次复制期间，内核暂时启用 supervisor PAE 映射，并在返回调用程序前恢复原先的页目录。进程退出会回收尚未释放的句柄。

## 验证

安装盘上的 16 位程序 `MEMSTAT /HIGH` 会分配两页，比较写入值与独立页的内容，并检查越界、释放、清零复用和失效句柄。运行验收脚本前，按 README 的原生安装盘构建命令生成最新工具链、驱动和 MEMSTAT。

```sh
python3 scripts/realmode-high-memory.py --image /path/to/install.img \
  --ram-dir /path/to/ramdisk --proof target/native16/high-memory
```

脚本在 RAM 盘上安装并冷启动 FAT16 C 盘。标准 QEMU i386 和 x86_64 机型将 64 MiB 放在低地址，32 MiB 放在 4 GiB 以上。DOS 读回测试后，脚本使用 QEMU 的 `pmemsave` 从页的实际物理地址取出相同四个字节，同时检查低地址处的原始内容。每次启动都接着运行 `MEMSTAT /TEST`，覆盖 EMS、XMS、UMB 和 VCPI。

标准机型验收使用 QEMU 11.1.2。可另传 `--dosbox-x /path/to/dosbox-x` 检查 80286 和 80386 冷启动、查询和返回提示符。

另有一个物理地址最高位的边界试验。标准 PC 机型的热插拔 DIMM 由 ACPI 描述。为了使用原生 BIOS 的 E820 启动路径，测试用 QEMU 10.0.12 UTM 源码将普通高 RAM 的物理起点放在 128 TiB。这个试验使用原有 SeaBIOS、同一 DOS 驱动和同一 C 盘。补丁只改变 QEMU 机型的内存起点。[QEMU 测试机单行补丁](../tests/fixtures/qemu/bit47-ram.patch)

```sh
# 在单独的 QEMU 源码树应用补丁并构建 x86_64-softmmu
patch -p1 < /path/to/DOS-RS/tests/fixtures/qemu/bit47-ram.patch
mkdir build-bit47
cd build-bit47
../configure --target-list=x86_64-softmmu --without-default-features --enable-tcg --enable-pixman
ninja qemu-system-x86_64

# 切回 DOS-RS，先运行标准机型，再运行上述边界试验
python3 scripts/realmode-high-memory.py --image /path/to/install.img \
  --ram-dir /path/to/ramdisk --proof target/native16/high-memory \
  --bit47-qemu /path/to/build-bit47/qemu-system-x86_64 --firmware-dir /path/to/qemu/firmware
```
