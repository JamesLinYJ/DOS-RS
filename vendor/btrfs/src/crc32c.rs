//! CRC-32C without a host CPU detection or operating-system dependency.
const TABLE: [u32; 256] = {
    let mut table = [0; 256];
    let mut i = 0;
    while i < table.len() {
        let mut value = i as u32;
        let mut bit = 0;
        while bit < 8 {
            value = (value >> 1) ^ (0x82f63b78 & 0u32.wrapping_sub(value & 1));
            bit += 1;
        }
        table[i] = value;
        i += 1;
    }
    table
};
pub fn crc32c_append(seed: u32, bytes: &[u8]) -> u32 {
    let mut state = !seed;
    for &byte in bytes {
        state = TABLE[((state as u8) ^ byte) as usize] ^ (state >> 8);
    }
    !state
}
pub fn crc32c(bytes: &[u8]) -> u32 { crc32c_append(0, bytes) }
