[bits 16]
[org 0x7c00]

start:
    jmp 0:init

init:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7c00
    sti

    mov [boot_drive], dl

    ; Print greeting
    mov si, msg_welcome
    call print_string

    ; Load Stage 2
    ; For now, we assume Stage 2 starts at LBA 1 and is 32 sectors long
    ; (This would be patched by an installer in a real scenario)
    mov eax, [stage2_lba]
    mov bx, 0x8000       ; Load to 0x8000
    mov cx, [stage2_size] ; sectors
    call read_sectors

    ; Jump to Stage 2
    mov si, msg_jumping
    call print_string
    
    mov dl, [boot_drive]
    jmp 0x0000:0x8000   ; FAR JUMP to Stage 2

halt:
    cli
    hlt
    jmp halt

; si: pointer to string
print_string:
    mov ah, 0x0e
.loop:
    lodsb
    test al, al
    jz .done
    int 0x10
    jmp .loop
.done:
    ret

; eax: lba, bx: dest (es:bx), cx: count
read_sectors:
    pusha
    mov [dap_lba], eax
    mov [dap_count], cx
    mov [dap_offset], bx
    mov [dap_segment], es
    
    mov ah, 0x42         ; Extended Read
    mov dl, [boot_drive]
    mov si, dap
    int 0x13
    jc .error
    popa
    ret
.error:
    mov si, msg_err_read
    call print_string
    jmp halt

boot_drive db 0
msg_welcome db 'Zamak BIOS Stage 1', 13, 10, 0
msg_jumping db 'Jumping to Stage 2...', 13, 10, 0
msg_err_read db 'Disk Read Error!', 13, 10, 0

; Disk Address Packet (DAP)
align 4
dap:
    db 0x10             ; size of DAP
    db 0                 ; reserved
dap_count:
    dw 0                 ; count
dap_offset:
    dw 0x8000            ; offset
dap_segment:
    dw 0x0000            ; segment
dap_lba:
    dq 1                 ; LBA (64-bit)

; Patchable fields
times 440-($-$$) db 0
stage2_lba dd 1         ; LBA of Stage 2 at offset 440
stage2_size dw 32       ; Size in sectors at offset 444

; Partition Table (16 bytes * 4) at offset 446
times 446-($-$$) db 0   

; Partition Table (16 bytes * 4)
times 64 db 0

dw 0xaa55               ; Boot Signature
