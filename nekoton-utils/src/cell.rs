const EMPTY_CELL_HASH: [u8; 32] = [
    0x96, 0xa2, 0x96, 0xd2, 0x24, 0xf2, 0x85, 0xc6, 0x7b, 0xee, 0x93, 0xc3, 0x0f, 0x8a, 0x30, 0x91,
    0x57, 0xf0, 0xda, 0xa3, 0x5d, 0xc5, 0xb8, 0x7e, 0x41, 0x0b, 0x78, 0x63, 0x0a, 0x09, 0xcf, 0xc7,
];

pub fn is_empty_cell(code_hash: &ton_types::UInt256) -> bool {
    code_hash.as_slice() == &EMPTY_CELL_HASH
}
