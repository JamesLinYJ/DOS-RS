pub const COMMAND: u16 = 0x3800;
pub const PROGRAM: u16 = 0x4000;
pub const CURRENT_PSP: u16 = 280;
pub const FONT_META: u16 = 0x37c0;
pub const FONT_WORKSPACE_SEGMENT: u16 = 28;

/// Segment of the final conventional MCB, immediately below the font workspace.
pub const fn arena_limit(base_kib: u16) -> u16 {
    if base_kib >= 560 { 0x82ff } else { 0x6fff }
}
