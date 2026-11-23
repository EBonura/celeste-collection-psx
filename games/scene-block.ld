/* The handoff block (CORE-ABI.md section 2), named in the link map so the harness and the
   journeys can read it by symbol (`SCENE_BLOCK` + offset). NOLOAD: the image carries no bytes for
   it and `_start` does not clear it. Linked after psoxide.ld, so psoxide.ld's IMAGE stays the
   first memory region (the one sections without a region of their own, such as
   psoxide-boot.ld's, go to). */
MEMORY {
    HANDOFF (rw) : ORIGIN = 0x801FFF00, LENGTH = 0x100
}
SECTIONS {
    .scene_block (NOLOAD) : { KEEP(*(.scene_block)) } > HANDOFF
}
