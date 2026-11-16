//! Handle ownership and four 16 KiB LIM page registers. Physical pages need not be contiguous.
pub const SLOTS: usize = 4;
const HANDLES: usize = 32;
pub const MAX: usize = 256;
#[derive(Copy, Clone, PartialEq)]
pub struct Page {
    pub handle: u16,
    pub logical: u16,
}
pub const EMPTY: Page = Page {
    handle: 0,
    logical: 0,
};
#[derive(Copy, Clone)]
struct Handle {
    count: u16,
    saved: Option<[Page; SLOTS]>,
}
pub struct Manager {
    total: u16,
    handles: [Handle; HANDLES],
    owner: [Page; MAX],
    pub map: [Page; SLOTS],
}
impl Manager {
    pub const fn new() -> Self {
        Self {
            total: 0,
            handles: [Handle {
                count: 0,
                saved: None,
            }; HANDLES],
            owner: [EMPTY; MAX],
            map: [EMPTY; SLOTS],
        }
    }
    pub fn init(&mut self, total: u16) {
        self.total = total.min(MAX as u16);
    }
    pub fn total(&self) -> u16 {
        self.total
    }
    pub fn free(&self) -> u16 {
        self.owner[..self.total as usize]
            .iter()
            .filter(|p| **p == EMPTY)
            .count() as u16
    }
    pub fn alloc(&mut self, n: u16) -> Result<u16, u8> {
        if n == 0 {
            return Err(0x89);
        }
        if n > self.total {
            return Err(0x87);
        }
        if n > self.free() {
            return Err(0x88);
        }
        let id = (1..HANDLES)
            .find(|&i| self.handles[i].count == 0)
            .ok_or(0x85u8)?;
        for (i, p) in self.owner[..self.total as usize]
            .iter_mut()
            .filter(|p| **p == EMPTY)
            .take(n as usize)
            .enumerate()
        {
            *p = Page {
                handle: id as u16,
                logical: i as u16,
            };
        }
        self.handles[id] = Handle {
            count: n,
            saved: None,
        };
        Ok(id as u16)
    }
    fn handle(&self, id: u16) -> Result<&Handle, u8> {
        self.handles
            .get(id as usize)
            .filter(|h| h.count != 0)
            .ok_or(0x83)
    }
    pub fn physical(&self, page: Page) -> Option<usize> {
        self.owner[..self.total as usize]
            .iter()
            .position(|p| *p == page)
    }
    pub fn assign(&mut self, slot: u16, id: u16, logical: u16) -> Result<(), u8> {
        if slot >= 4 {
            return Err(0x8b);
        }
        let n = self.handle(id)?.count;
        self.map[slot as usize] = if logical == 0xffff {
            EMPTY
        } else if logical < n {
            Page {
                handle: id,
                logical,
            }
        } else {
            return Err(0x8a);
        };
        Ok(())
    }
    pub fn release(&mut self, id: u16) -> Result<(), u8> {
        if self.handle(id)?.saved.is_some() {
            return Err(0x86);
        }
        for p in self.owner.iter_mut().chain(self.map.iter_mut()) {
            if p.handle == id {
                *p = EMPTY;
            }
        }
        // A saved mapping cannot later resurrect a released page or a recycled handle.
        for h in &mut self.handles {
            if let Some(map) = &mut h.saved {
                for p in map {
                    if p.handle == id {
                        *p = EMPTY;
                    }
                }
            }
        }
        self.handles[id as usize].count = 0;
        Ok(())
    }
    pub fn save(&mut self, id: u16) -> Result<(), u8> {
        if self.handle(id)?.saved.is_some() {
            return Err(0x8d);
        }
        self.handles[id as usize].saved = Some(self.map);
        Ok(())
    }
    pub fn restore(&mut self, id: u16) -> Result<(), u8> {
        self.handle(id)?;
        self.map = self.handles[id as usize].saved.take().ok_or(0x8eu8)?;
        Ok(())
    }
    pub fn count(&self, id: u16) -> Result<u16, u8> {
        Ok(self.handle(id)?.count)
    }
    pub fn allocations(&self) -> impl Iterator<Item = (u16, u16)> + '_ {
        self.handles
            .iter()
            .enumerate()
            .filter(|(_, h)| h.count != 0)
            .map(|(i, h)| (i as u16, h.count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_and_saved_maps_follow_handle_lifetime() {
        let mut m = Manager::new();
        m.init(5);
        let first = m.alloc(3).unwrap();
        let second = m.alloc(2).unwrap();
        m.assign(0, first, 2).unwrap();
        m.assign(3, first, 2).unwrap();
        assert_eq!(m.physical(m.map[0]), m.physical(m.map[3]));
        m.save(second).unwrap();
        assert_eq!(m.release(second), Err(0x86));
        m.release(first).unwrap();
        assert_eq!(m.alloc(1), Ok(first));
        m.restore(second).unwrap();
        assert!(m.map.iter().all(|p| *p == EMPTY));
        assert_eq!(m.free(), 2);
    }

    #[test]
    fn invalid_requests_leave_maps_and_accounting_intact() {
        let mut m = Manager::new();
        m.init(3);
        assert_eq!(m.alloc(0), Err(0x89));
        assert_eq!(m.alloc(4), Err(0x87));
        let id = m.alloc(2).unwrap();
        assert_eq!(m.alloc(2), Err(0x88));
        m.assign(2, id, 1).unwrap();
        let original = m.map;
        assert_eq!(m.assign(2, id, 2), Err(0x8a));
        assert_eq!(m.assign(4, id, 1), Err(0x8b));
        assert!(m.map == original);
        m.assign(2, id, 0xffff).unwrap();
        assert!(m.map[2] == EMPTY);
        m.release(id).unwrap();
        assert_eq!(m.free(), 3);
        assert_eq!(m.count(id), Err(0x83));
    }
}
