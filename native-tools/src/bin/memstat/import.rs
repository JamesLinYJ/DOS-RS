use super::{DOS, Regs, XMS, call, ems, number, store, text, xms};

fn import_after_xms_allocation(regs: &mut Regs, fd: u16, descriptor: &mut [u16; 3]) -> bool {
    if !ems(regs, 0x4100, 0, 0) {
        return false;
    }
    let frame = regs[1];
    if !ems(regs, 0x4300, 2, 0) {
        return false;
    }
    let expanded = regs[3];
    let mut ok = true;
    let mut page = 0;
    while page < 2 {
        if ems(regs, 0x4400, page, expanded) {
            store(frame, 0, 0x61 + page);
        } else {
            ok = false;
        }
        page += 1;
    }
    let mut held = [0; 32];
    let mut n = 0;
    loop {
        call(regs, XMS, 0x0800, 0, 0);
        let kib = regs[0];
        if kib == 0 {
            break;
        }
        if n == held.len() || !xms(regs, 0x0900, kib) {
            ok = false;
            break;
        }
        held[n] = regs[3];
        n += 1;
    }
    descriptor[0] = 1;
    regs[2] = 6;
    call(regs, DOS, 0x4402, fd, descriptor.as_mut_ptr() as usize as u16);
    ok = ok && n != 0 && regs[10] & 1 == 0 && regs[0] == 6 && regs[2] == 6;
    while n != 0 {
        n -= 1;
        ok = xms(regs, 0x0a00, held[n]) && ok;
    }
    ems(regs, 0x4500, 0, expanded) && ok
}

pub(super) fn test(regs: &mut Regs) -> u16 {
    let mut name = *b"EMMXXXX0\0";
    call(regs, DOS, 0x3d00, 0, name.as_mut_ptr() as usize as u16);
    if regs[10] & 1 != 0 {
        return 1;
    }
    let fd = regs[0];
    call(regs, DOS, 0x4400, fd, 0);
    let mut ok = regs[10] & 1 == 0 && regs[3] & 0x4080 == 0x4080;
    call(regs, DOS, 0x4407, fd, 0);
    ok = ok && regs[10] & 1 == 0 && regs[0] & 255 != 0;
    let mut descriptor = [0u16; 3];
    ok = import_after_xms_allocation(regs, fd, &mut descriptor) && ok;
    let physical = u32::from(descriptor[0]) | u32::from(descriptor[1]) << 16;
    ok = ok && physical != 0 && descriptor[2] == 1;
    call(regs, DOS, 0x3e00, fd, 0);
    if ok && regs[10] & 1 == 0 {
        text("GEMMIS 1.0 physical address ");
        number(physical);
        text("\r\n");
        0
    } else {
        text("EMM import test FAILED\r\n");
        1
    }
}
