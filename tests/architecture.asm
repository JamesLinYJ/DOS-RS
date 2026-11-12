; Build with nasm -f bin tests/architecture.asm -o tests/architecture.com
cpu 8086
bits 16
org 100h

; Exercise the interpreter's real INT instruction and preserve incoming flags.
mov dx, 0xbeef
mov si, 0x9876
mov di, 0x1357
mov bp, 0x2468
stc
call version
jnc bad_version
clc
call version
jc bad_version
cmp dx, 0xbeef
jne bad_version
cmp si, 0x9876
jne bad_version
cmp di, 0x1357
jne bad_version
cmp bp, 0x2468
jne bad_version

; Allocate above the running PSP, then patch a future instruction via a segment alias.
mov bx, 0x20
mov ah, 48h
int 21h
jc bad_memory
cmp ax, [2]
jb bad_memory
mov [heap], ax
mov es, ax
mov byte [es:0], 0x79
mov ax, ds
add ax, 10h
mov es, ax
mov byte [es:patched+1-100h], 0x2a
jmp patched

version:
mov ax, 30a5h
mov bx, 0x1234
mov cx, 0x5678
int 21h
pushf
cmp ax, 4
jne bad_version
cmp bx, 0xff00
jne bad_version
or cx, cx
jnz bad_version
popf
ret

bad_version: mov ax, 4c01h
int 21h
bad_memory: mov ax, 4c02h
int 21h
bad_exec: mov ax, 4c03h
int 21h
bad_parent: mov ax, 4c04h
int 21h

; Offset beyond 1000h reaches the region formerly hidden behind shadow RAM.
times 1000h-($-$$) db 0
patched:
mov al, 0
cmp al, 0x2a
jne bad_memory

; Create an executable on the mounted volume, then launch it through AH=4Bh.
mov dx, filename
xor cx, cx
mov ah, 3ch
int 21h
jc bad_exec
mov bx, ax
mov dx, child
mov cx, child_end-child
mov ah, 40h
int 21h
jc bad_exec
cmp ax, child_end-child
jne bad_exec
mov ah, 3eh
int 21h
jc bad_exec
push ds
pop es
mov dx, dta
mov ah, 1ah
int 21h
mov dx, filename
mov bx, params
mov ax, 4b00h
int 21h
jc bad_exec
mov ah, 4dh
int 21h
cmp ax, 37
jne bad_exec
mov ah, 2fh
int 21h
cmp bx, dta
jne bad_parent
mov ax, es
mov bx, ds
cmp ax, bx
jne bad_parent
mov es, [heap]
cmp byte [es:0], 0x79
jne bad_parent
mov ah, 49h
int 21h
jc bad_parent
mov ah, 49h
int 21h
jnc bad_parent
cmp ax, 9
jne bad_parent
mov ax, 4c00h
int 21h

heap: dw 0
filename: db 'ARCHKID.COM',0
params: times 7 dw 0
dta: times 43 db 0

; This range becomes a separate COM file at offset 100h in the child.
child:
cmp byte [80h], 0
jne .recurse
mov ah, 2fh
int 21h
cmp bx, 80h
jne .bad
mov ax, es
mov bx, ds
cmp ax, bx
jne .bad
mov es, [16h]
mov ax, [2ch]
cmp ax, [es:2ch]
je .bad
mov es, [es:heap]
mov ah, 49h
int 21h
jnc .bad
cmp ax, 9
jne .bad
mov dx, 200h
mov ah, 1ah
int 21h
mov bx, 0x100
mov ah, 48h
int 21h
jc .bad
.recurse:
; Each invocation names the same child on the live volume. Count the returns
; to verify that the final failed EXEC unwinds all eight suspended children.
push ds
pop es
mov [100h + .block-child + 4], ds
mov bx, 100h + .block-child
mov dx, 100h + .name-child
mov ax, 4b00h
int 21h
jnc .returned
cmp ax, 8
jne .bad
mov ax, 4c01h
int 21h
.returned:
mov ah, 4dh
int 21h
cmp byte [80h], 0
je .outer
inc al
mov ah, 4ch
int 21h
.outer:
cmp al, 7
jne .bad
mov ax, 4c25h
int 21h
.name: db 'ARCHKID.COM',0
.tail: db 1,'D',13
.block: dw 0,100h + .tail-child,0,0,0,0,0
.bad: mov ax, 4c80h
int 21h
child_end:
