//! The EXE's initial PC: the mover of the boot-only overlay (the SDK's psoxide-boot.ld, laid out by
//! host/code_modules.py `boot_layer`).
//!
//! The overlay (the PSoXide boot splash and the code only it calls) is stored in the EXE right after
//! `.data`, where `.bss` will be, and runs from the start of the module pool, which nothing uses until
//! the title module installs. `__boot_entry` copies it up to the pool (from the end backwards), zeroes
//! its own `.bss` part and jumps to psx-rt's `_start`, which then clears `.bss` over the stored copy.
//! It uses no stack and leaves the loader's argument registers (`$a0..$a2`) alone. Nothing of the
//! overlay stays resident: the title module's install takes the pool bytes back.

#[cfg(target_arch = "mips")]
core::arch::global_asm!(
    r#"
    .section .text.__boot_entry,"ax",@progbits
    .globl __boot_entry
    .type __boot_entry, @function
    .set push
    .set noreorder
    .set noat
__boot_entry:
    lui   $t0, %hi(__boot_lma)
    addiu $t0, $t0, %lo(__boot_lma)
    lui   $t1, %hi(__boot_start)
    addiu $t1, $t1, %lo(__boot_start)
    lui   $t2, %hi(__boot_load_end)
    addiu $t2, $t2, %lo(__boot_load_end)
    subu  $t3, $t2, $t1
    addu  $t4, $t0, $t3
1:  beq   $t2, $t1, 2f
    nop
    addiu $t4, $t4, -4
    lw    $t5, 0($t4)
    addiu $t2, $t2, -4
    b     1b
    sw    $t5, 0($t2)
2:  lui   $t1, %hi(__boot_load_end)
    addiu $t1, $t1, %lo(__boot_load_end)
    lui   $t2, %hi(__boot_end)
    addiu $t2, $t2, %lo(__boot_end)
3:  beq   $t1, $t2, 4f
    nop
    sw    $zero, 0($t1)
    b     3b
    addiu $t1, $t1, 4
4:  j     _start
    nop
    .size __boot_entry, . - __boot_entry
    .set pop
"#
);
