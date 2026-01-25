[bits 16]
section .entry

global _start
_start:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x8000

    ; Load GDT
    lgdt [gdt_descriptor]

    ; Enter Protected Mode
    mov eax, cr0
    or eax, 1
    mov cr0, eax

    ; Far jump to 32-bit code
    jmp 0x08:init_32

[bits 32]
init_32:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    ; Call Rust main
    extern kmain
    and edx, 0xFF        ; Ensure only dl is set
    push edx             ; Pass drive_id to kmain
    call kmain

.halt:
    hlt
    jmp .halt

; void call_bios_int(uint8_t int_no, bios_regs* regs)
global call_bios_int
call_bios_int:
    push ebp
    mov ebp, esp
    pusha

    ; Save stack pointer
    mov [esp_save_ptr], esp

    ; Transition to 16-bit protected mode
    jmp 0x18:.pm16

[bits 16]
.pm16:
    mov ax, 0x20
    mov ds, ax
    mov es, ax
    mov ss, ax

    ; Disable protected mode
    mov eax, cr0
    and eax, ~1
    mov cr0, eax

    ; Jump to real mode
    jmp 0x00:.rm

.rm:
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7000 ; Use a safe real-mode stack

    ; Set up registers for interrupt
    mov eax, [ebp + 12] ; regs pointer
    mov edi, eax
    
    mov eax, [edi + 0]  ; eax
    mov ebx, [edi + 4]  ; ebx
    mov ecx, [edi + 8]  ; ecx
    mov edx, [edi + 12] ; edx
    mov esi, [edi + 16] ; esi
    push dword [edi + 20] ; push edi to stack temporarily
    
    ; The interrupt number is at [ebp + 8]
    ; SELF-MODIFYING CODE HACK because we can't easily do dynamic interrupt calls in real mode
    mov al, [ebp + 8]
    mov [int_op + 1], al
    
    pop edi ; restore edi

int_op:
    int 0               ; Modified at runtime

    ; Save results
    push edi
    mov edi, [ebp + 12]
    mov [edi + 0], eax
    mov [edi + 4], ebx
    mov [edi + 8], ecx
    mov [edi + 12], edx
    mov [edi + 16], esi
    pop eax
    mov [edi + 20], eax ; save edi

    ; Back to protected mode
    mov eax, cr0
    or eax, 1
    mov cr0, eax

    ; Jump to 32-bit protected mode
    jmp 0x08:.pm32

[bits 32]
.pm32:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    
    ; Restore 32-bit stack
    mov esp, [esp_save_ptr]
    popa
    pop ebp
    ret

align 4
gdt_start:
    dq 0x0000000000000000 ; Null descriptor
    dq 0x00cf9a000000ffff ; 0x08: Code 32 (0..4G, P, R, E)
    dq 0x00cf92000000ffff ; 0x10: Data 32 (0..4G, P, W)
    dq 0x00009a000000ffff ; 0x18: Code 16 (Real-mode compatible)
    dq 0x000092000000ffff ; 0x20: Data 16
    dq 0x00af9a000000ffff ; 0x28: Code 64 (Long Mode)
gdt_end:

[bits 32]
; void enter_long_mode(uint32_t pml4_phys, uint64_t entry_point)
global enter_long_mode
enter_long_mode:
    mov eax, [esp + 4]  ; pml4_phys
    mov cr3, eax

    ; Enable PAE
    mov eax, cr4
    or eax, 1 << 5
    mov cr4, eax

    ; Enable Long Mode in EFER MSR
    mov ecx, 0xC0000080 ; EFER MSR
    rdmsr
    or eax, 1 << 8      ; LME bit
    wrmsr

    ; Enable Paging
    mov eax, cr0
    or eax, 1 << 31
    mov cr0, eax

    ; Far jump to 64-bit code segment
    jmp 0x28:init_64

[bits 64]
init_64:
    ; We are now in 64-bit mode!
    ; The entry point (u64) is at [rsp + 8] for 32-bit calling convention?
    ; Wait, we are in 64-bit now. The 32-bit stack is still there.
    ; But we should jump to the entry point.
    ; The entry point in 32-bit was passed as two 32-bit args: [esp+8] (low) and [esp+12] (high)
    ; Since we are in 64-bit, we can just use rbx or something.
    
    ; Actually, let's go back to 32-bit code to get the entry point before the jump.
    ; No, let's just pass it correctly.
    
    ; Let's re-save the entry point in a known location before entering long mode.
    ; Or just pop it into a 64-bit register before enabling paging?
    ; No, rdmsr/wrmsr use eax/edx.
    
    ; Let's assume RSI/RDI? No, let's just use a fixed memory location.
    mov rbx, [0x5FF0] ; We'll store it here in Rust
    jmp rbx

gdt_descriptor:
    dw gdt_end - gdt_start - 1
    dd gdt_start

section .data
align 4
esp_save_ptr dd 0
