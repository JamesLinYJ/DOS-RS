//! Interrupt routing for a DOS session that installs its own protected-mode handlers.
use alloc::sync::Arc;
use rsemu::core::{
    Device,
    space::{AddressSpace, MemAttrs},
    value::Width,
    wire::{IntAckCycle, Wire, WireIdAllocator, WireSource},
};
use rsemu::dev::pc::{kbc::Kbc8042, pic::Pic8259, pit::Pit8254};
use rsemu::host::chardev::CharPort;

#[derive(Debug)]
pub(super) struct At {
    firmware: AddressSpace,
    pic: Pic8259,
    timer: Pit8254,
    keyboard: Kbc8042,
    input: Arc<CharPort>,
}
impl At {
    pub fn new() -> Self {
        let pic = Pic8259::default_device();
        let timer = Pit8254::default_device();
        let input = Arc::new(CharPort::new());
        let keyboard = Kbc8042::with_port(input.clone(), "terminal".into());
        let ids = WireIdAllocator::new();
        let wire = |name| {
            let id = ids.alloc();
            let pin = pic.sink(name, &[id]).unwrap();
            WireSource::new(
                Wire::builder()
                    .source(id)
                    .sink(pin.sink, pin.line)
                    .build_shared(),
                id,
            )
        };
        timer.connect("out0", wire("ir0")).unwrap();
        keyboard.connect("irq1", wire("ir1")).unwrap();
        let dev = Self {
            pic,
            timer,
            keyboard,
            input,
            firmware: AddressSpace::new("AT firmware", 16),
        };
        let io = &dev.firmware;
        dev.map(io);
        for (port, value) in [
            (0x20, 0x11),
            (0x21, 8),
            (0x21, 4),
            (0x21, 1),
            (0x21, 0xfc),
            (0x43, 0x36),
            (0x40, 0),
            (0x40, 0),
            (0x64, 0x60),
            (0x60, 0x41),
        ] {
            io.write(port, Width::U8, value, MemAttrs::DEFAULT).unwrap();
        }
        dev
    }
    pub fn map(&self, io: &AddressSpace) {
        let mut top = io.topology();
        for (dev, name, addr) in [
            (&self.pic as &dyn Device, "regs", 0x20),
            (&self.timer, "regs", 0x40),
            (&self.keyboard, "data", 0x60),
            (&self.keyboard, "cmd", 0x64),
        ] {
            top.map_with_priority(dev.region(name).unwrap(), addr, 1)
                .unwrap();
        }
    }
    pub fn key(&self, scan: u8) {
        // The host returns set 1. Feed the guest controller set 2 so Windows
        // can independently enable or disable its controller's translation.
        if matches!(scan, 0xe0 | 0xe1) {
            self.input.feed(&[scan]);
        } else {
            let make = scan & 0x7f;
            let table = &rsemu::dev::pc::kbc::TRANSLATE[..0x87];
            let set2 = if make == 0x41 {
                0x83
            } else if let Some(index) = table.iter().position(|&v| v == make) {
                index as u8
            } else {
                return;
            };
            if scan & 0x80 != 0 {
                self.input.feed(&[0xf0, set2]);
            } else {
                self.input.feed(&[set2]);
            }
        }
        self.keyboard.pump();
    }
    pub fn advance(&self, ticks: u64) {
        self.timer.advance_to(self.timer.tick().wrapping_add(ticks));
        self.keyboard.pump();
    }
    pub fn interrupt(&self) -> Option<u8> {
        if !self.pic.int_asserted() {
            return None;
        }
        self.pic
            .int_ack("int")?
            .acknowledge(IntAckCycle::vector_only())
            .vector()
            .map(|v| v as u8)
    }
    pub fn eoi(&self) {
        self.firmware
            .write(0x20, Width::U8, 0x20, MemAttrs::DEFAULT)
            .unwrap();
    }
}
