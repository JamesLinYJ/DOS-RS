pub const COMMAND: u16 = 0x3800;
pub const PROGRAM: u16 = 0x4000;
pub const CURRENT_PSP: u16 = 280;
// Segment of the interpreter currently executing in its split MZ layout.
// The paragraph immediately following that PSP describes its live SS=DS.
#[cfg(not(all(target_pointer_width = "32", target_os = "none")))]
pub const ACTIVE_SHELL: u16 = 0x20a;
// IO.SYS retains the DOSK image descriptors here until kernel startup. Only
// the first word is reused for individual compressed block lengths.
#[cfg(not(all(target_pointer_width = "32", target_os = "none")))]
pub const KERNEL_HEADER: u16 = 0x3280;
#[cfg(not(all(target_pointer_width = "32", target_os = "none")))]
pub const KERNEL_LIMIT: u16 = 0x2e00;
pub const FONT_META: u16 = 0x37c0;
pub const FONT_WORKSPACE_SEGMENT: u16 = 28;

/// Segment of the final conventional MCB, immediately below the font workspace.
pub const fn arena_limit(base_kib: u16) -> u16 {
    if base_kib >= 560 { 0x82ff } else { 0x6fff }
}

// Kernel-to-EMM requests over the native DOS memory multiplex.
pub const UMB_EXEC: u16 = 0xf001;
pub const UMB_SET_OWNER: u16 = 0xf002;
pub const UMB_OWNS_RANGE: u16 = 0xf003;
