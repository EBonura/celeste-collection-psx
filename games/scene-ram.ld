/* The top of RAM the collection links for: one build parameter. Everything above it is the scene
   core's window (8,192 B, provisional) and the handoff block (0x801FFF00..0x80200000), see the scene
   system's CORE-ABI.md section 0. psoxide.ld puts STACK_INIT 100h below it (0x801FDF00). The
   collection's build script reads this line and passes the file as `-T` ahead of psoxide.ld, so the
   link map records it. `PSX_RAM_SIZE_OVERRIDE=0x...` in the environment replaces the value for one
   build. */
RAM_SIZE_OVERRIDE = 0x1FE000;
