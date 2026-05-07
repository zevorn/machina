.section .text
.globl _start
_start:
    lui     a0, 0x100       # a0 = 0x100000
    li      a1, 0x7777      # a1 = RESET
    sw      a1, 0(a0)       # trigger reset
hang:
    wfi
    j       hang
