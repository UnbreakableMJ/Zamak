[bits 16]
[org 0x1000]

start:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    
    ; Load temporary GDT
    lgdt [gdt_ptr]
    
    ; Enable Protected Mode
    mov eax, cr0
    or eax, 1
    mov cr0, eax
    
    ; Jump to 32-bit code
    jmp 0x08:pm_start

[bits 32]
pm_start:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    
    ; Setup PAE and Long Mode
    mov eax, cr4
    or eax, 1 << 5      ; PAE
    mov cr4, eax
    
    ; Setup Paging (CR3)
    ; This must be the same PML4 as the BSP (to be patched at runtime)
    mov eax, [pml4_ptr]
    mov cr3, eax
    
    ; Enable Long Mode in EFER MSR
    mov ecx, 0xC0000080 ; EFER MSR
    rdmsr
    or eax, 1 << 8      ; LME
    wrmsr
    
    ; Enable Paging
    mov eax, cr0
    or eax, 1 << 31     ; PG
    mov cr0, eax
    
    ; Jump to 64-bit code
    lgdt [gdt_ptr_long]
    jmp 0x08:long_start

[bits 64]
long_start:
    ; We are now in 64-bit mode
    ; Park the AP and wait for instructions
    
.wait_loop:
    ; Wait for a transition to be requested by the kernel or bootloader
    ; In Limine, APs wait for 'goto_address' to be non-zero
    ; But we need to know WHICH CpuInfo we are.
    ; Usually, we use the LAPIC ID to find our CpuInfo block.
    
    ; For now, just halt
    hlt
    jmp .wait_loop

align 8
gdt:
    dq 0x0000000000000000 ; Null
    dq 0x00cf9a000000ffff ; Code 32
    dq 0x00cf92000000ffff ; Data 32
gdt_ptr:
    dw $ - gdt - 1
    dd gdt

gdt_long:
    dq 0x0000000000000000 ; Null
    dq 0x00af9a000000ffff ; Code 64
    dq 0x00af92000000ffff ; Data 64
gdt_ptr_long:
    dw $ - gdt_long - 1
    dq gdt_long

; Patchable fields at fixed offsets
times 0x500 - ($ - $$) db 0
pml4_ptr dd 0          ; Offset 0x500
entry_point dq 0       ; Offset 0x504
stack_top dq 0         ; Offset 0x50C
