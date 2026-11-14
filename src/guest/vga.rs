//! Per-session VGA graphics registers, plane latches and scanout.
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering::Relaxed};
use rsemu::core::Device;
use rsemu::core::space::{AddressSpace, MemAttrs, MemOps, MemResult, Region};
use rsemu::core::value::Width;
use rsemu::dev::pc::video::Video;
use rsemu::host::display::{Scanout, Surface};

pub(super) struct Graphics {
    adapter: Arc<Video>,
    ports: Arc<Registers>,
    memory: AddressSpace,
    surface: Surface,
    pub active: bool,
    refresh: u16,
}

const PORTS: [(u16, &str); 5] = [
    (0x3c0, "vga"),
    (0x3b4, "crtc-mono"),
    (0x3d4, "crtc-colour"),
    (0x3d8, "mode"),
    (0x3da, "status"),
];

#[derive(Debug)]
struct Registers {
    adapter: Arc<Video>,
    bus: AddressSpace,
    changed: AtomicBool,
}
impl MemOps for Registers {
    fn read(&self, offset: u64, dst: &mut [u8], attrs: MemAttrs) -> MemResult {
        for (i, b) in dst.iter_mut().enumerate() {
            let port = 0x3b0 + offset + i as u64;
            if port == 0x3da && !attrs.debug {
                self.adapter
                    .advance_to(self.adapter.ticks().wrapping_add(97));
            }
            *b = self.bus.read(port, Width::U8, attrs).unwrap_or(255) as u8;
        }
        Ok(())
    }
    fn write(&self, offset: u64, src: &[u8], attrs: MemAttrs) -> MemResult {
        for (i, b) in src.iter().enumerate() {
            let _ = self
                .bus
                .write(0x3b0 + offset + i as u64, Width::U8, (*b).into(), attrs);
        }
        self.changed.store(true, Relaxed);
        Ok(())
    }
}

impl Graphics {
    pub fn new() -> Self {
        let adapter = Arc::new(Video::vga(256 << 10, 0));
        let io = AddressSpace::new("guest VGA ports", 16);
        let memory = AddressSpace::new("guest VGA aperture", 32);
        for (base, name) in PORTS {
            io.topology()
                .map(adapter.region(name).unwrap(), base.into())
                .unwrap();
        }
        memory
            .topology()
            .map(adapter.region("window").unwrap(), 0xa0000)
            .unwrap();
        let ports = Arc::new(Registers {
            adapter: adapter.clone(),
            bus: io,
            changed: AtomicBool::new(true),
        });
        Self {
            adapter,
            ports,
            memory,
            surface: Surface::empty(),
            active: false,
            refresh: 0,
        }
    }

    pub fn attach(&self, memory: &AddressSpace, io: &AddressSpace) {
        // B800:0000 remains the shared text-page backing used by the BIOS and terminals.
        let aperture = Region::alias(
            "VGA planes",
            self.adapter.region("window").unwrap(),
            0,
            65536,
        )
        .unwrap();
        memory
            .topology()
            .map_with_priority(aperture, 0xa0000, 1)
            .unwrap();
        io.topology()
            .map_with_priority(Region::io("VGA ports", 0x30, self.ports.clone()), 0x3b0, 1)
            .unwrap();
    }

    pub fn write(&self, port: u16, byte: u8) {
        self.ports
            .write(u64::from(port - 0x3b0), &[byte], MemAttrs::DEFAULT)
            .unwrap();
    }
    pub fn port(&self, port: u16, write: bool, byte: u8) -> Option<u8> {
        if !matches!(port,0x3b4..=0x3b5|0x3c0..=0x3cf|0x3d4..=0x3d5|0x3d8..=0x3da) {
            return None;
        }
        if write {
            self.write(port, byte);
            Some(0)
        } else {
            let mut out = [0];
            self.ports
                .read(u64::from(port - 0x3b0), &mut out, MemAttrs::DEFAULT)
                .unwrap();
            Some(out[0])
        }
    }
    pub fn byte(&self, addr: usize) -> u8 {
        self.memory
            .read(addr as u64, Width::U8, MemAttrs::DEFAULT)
            .unwrap_or(255) as u8
    }
    pub fn store(&self, addr: usize, value: u8) {
        let _ = self
            .memory
            .write(addr as u64, Width::U8, value.into(), MemAttrs::DEFAULT);
    }
    pub fn attribute(&self, index: u8, value: u8) {
        let _ = self.port(0x3da, false, 0);
        self.write(0x3c0, index);
        self.write(0x3c0, value);
        self.write(0x3c0, 0x20);
    }
    fn indexed(&self, port: u16, index: u8, value: u8) {
        self.write(port, index);
        self.write(port + 1, value);
    }
    pub fn planar_mode(&mut self, clear: bool) {
        // IBM VGA mode 12h, 640 x 480, four independently addressable bit planes.
        const CRTC: [u8; 25] = [
            0x5f, 0x4f, 0x50, 0x82, 0x54, 0x80, 0x0b, 0x3e, 0, 0x40, 0, 0, 0, 0, 0, 0, 0xea, 0x8c,
            0xdf, 0x28, 0, 0xe7, 4, 0xe3, 0xff,
        ];
        self.write(0x3c2, 0xe3);
        for (index, value) in [3, 1, 15, 0, 6].into_iter().enumerate() {
            self.indexed(0x3c4, index as u8, value);
        }
        self.indexed(0x3d4, 0x11, 0);
        for (index, value) in CRTC.into_iter().enumerate() {
            self.indexed(0x3d4, index as u8, value);
        }
        for (index, value) in [0, 0, 0, 0, 0, 0, 5, 15, 255].into_iter().enumerate() {
            self.indexed(0x3ce, index as u8, value);
        }
        let _ = self.port(0x3da, false, 0);
        for (index, value) in [
            0, 1, 2, 3, 4, 5, 0x14, 7, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 1, 0, 15, 0,
            0,
        ]
        .into_iter()
        .enumerate()
        {
            self.write(0x3c0, index as u8);
            self.write(0x3c0, value);
        }
        self.write(0x3c0, 0x20);
        self.write(0x3c6, 255);
        // The standard 64-entry EGA DAC cube (two bits per channel).
        self.write(0x3c8, 0);
        for i in 0..64u8 {
            for bit in [4, 2, 1] {
                let component = u8::from(i & bit != 0) * 42 + u8::from(i & (bit << 3) != 0) * 21;
                self.write(0x3c9, component);
            }
        }
        if clear {
            self.adapter.vram().fill(0, 256 << 10, 0).unwrap();
        }
        self.active = true;
        self.refresh = 0;
    }
    pub fn render(&mut self, force: bool) {
        if !self.active {
            return;
        }
        // Coalesce instruction slices; a terminal switch redraws immediately.
        self.refresh = self.refresh.wrapping_sub(1);
        if !force && self.refresh != u16::MAX {
            return;
        }
        self.refresh = 128;
        let changed = self.ports.changed.swap(false, Relaxed);
        if !force
            && !changed
            && !self.surface.is_empty()
            && self.adapter.vram().dirty_page_count() == 0
        {
            return;
        }
        self.adapter.vram().clear_dirty();
        let out = self.adapter.scanout();
        let info = out.info();
        if !(1..=1024).contains(&info.width) || !(1..=768).contains(&info.height) {
            return;
        }
        out.capture(&mut self.surface);
        crate::bios::screen::graphics(
            self.surface.width() as usize,
            self.surface.height() as usize,
            self.surface.pixels(),
        );
    }
}
