pub const COMMAND: u16 = 0x3800;
pub const PROGRAM: u16 = 0x4000;
pub const CURRENT_PSP: u16 = 280;
// Segment of the interpreter currently executing in its split MZ layout.
// The paragraph immediately following that PSP describes its live SS=DS.
pub const ACTIVE_SHELL: u16 = 0x20a;
// IO.SYS retains the DOSK image descriptors here until kernel startup. Only
// the first word is reused for individual compressed block lengths.
pub const KERNEL_HEADER: u16 = 0x3280;
pub const KERNEL_LIMIT: u16 = 0x2e00;
pub const FONT_META: u16 = 0x37c0;
pub const FONT_WORKSPACE_SEGMENT: u16 = 28;

/// Segment of the final conventional MCB, immediately below the font workspace.
pub const fn arena_limit(base_kib: u16) -> u16 {
    if base_kib >= 560 { 0x82ff } else { 0x6fff }
}
