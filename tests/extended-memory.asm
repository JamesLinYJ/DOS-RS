; A DOS client of the published XMS far-call and LIM EMS interrupt ABIs.
bits 16
org 100h
%macro xcall 1
    mov ah,%1
    call far [xms]
    cmp ax,1
    jne fail
%endmacro
%macro ecall 1
    mov ax,%1
    int 67h
    test ah,ah
    jnz fail
%endmacro
mov ax,4300h
int 2fh
cmp al,80h
jne fail
mov ax,4310h
int 2fh
mov [xms],bx
mov [xms+2],es
mov ah,88h
call far [xms]
mov [initial],edx
mov edx,256
xcall 89h
mov [xhandle],dx
xcall 0ch
mov dx,[xhandle]
mov ah,0ah
call far [xms]
cmp bl,0abh
jne fail
mov dx,[xhandle]
xcall 0dh
mov si,moveblock
mov [si+8],ds
mov ax,[xhandle]
mov [si+10],ax
xcall 0bh
mov si,readblock
mov [si+14],ds
mov ax,[xhandle]
mov [si+4],ax
xcall 0bh
cmp word [copy],05aa5h
jne fail
; Out-of-range offsets and transfers past a valid offset have distinct errors.
mov si,readblock
mov dword [si+6],0ffffffffh
mov ah,0bh
call far [xms]
cmp bl,0a4h
jne fail
mov dword [si+6],256*1024-1
mov ah,0bh
call far [xms]
cmp bl,0a7h
jne fail
mov dword [si+6],16
; Forward overlapping moves must preserve the bytes that have not yet been copied.
mov si,overlap
mov [si+8],ds
mov [si+14],ds
xcall 0bh
cmp word [sequence+4],3344h
jne fail
mov dx,[xhandle]
mov ebx,512
xcall 8fh
mov dx,[xhandle]
xcall 8eh
cmp edx,512
jne fail
mov dx,64
xcall 10h
mov [upper],bx
mov es,bx
mov word [es:2],1234h
mov dx,bx
mov bx,32
xcall 12h
cmp word [es:2],1234h
jne fail
; Link the DOS MCB chain, allocate high, and verify it cannot overlap the XMS UMB.
mov ax,5803h
mov bx,1
int 21h
jc fail
mov ax,5802h
int 21h
cmp al,1
jne fail
mov ax,5801h
mov bx,40h
int 21h
jc fail
mov ah,48h
mov bx,32
int 21h
jc fail
cmp ax,0c800h
jb fail
mov es,ax
mov word [es:2],5678h
mov cx,es
mov es,[upper]
cmp word [es:2],1234h
jne fail
mov ah,49h
int 21h
jnc fail
mov dx,cx
mov ah,11h
call far [xms]
cmp bl,0b2h
jne fail
mov es,cx
mov ah,49h
int 21h
jc fail
mov ax,5801h
xor bx,bx
int 21h
jc fail
mov ax,5803h
int 21h
jc fail
mov dx,[upper]
xcall 11h
mov dx,0ffffh
xcall 1
xcall 3
xcall 5
mov ah,4
call far [xms]
test ax,ax
jnz fail
cmp bl,94h
jne fail
xcall 7
mov ax,0ffffh
mov es,ax
mov word [es:18h],0beefh
xcall 6
xcall 2
mov bx,4
ecall 4300h
mov [emhandle],dx
ecall 4100h
mov es,bx
xor bx,bx
ecall 4400h
ecall 4401h
mov word [es:16],1234h
cmp word [es:4010h],1234h
jne fail
ecall 4700h
mov bx,1
ecall 4400h
mov word [es:16],4567h
ecall 4800h
cmp word [es:16],1234h
jne fail
mov bx,1
ecall 4401h
cmp word [es:4010h],4567h
jne fail
push cs
pop es
mov si,emove
mov [si+5],dx
mov [si+16],ds
ecall 5700h
cmp word [copy],4567h
jne fail
mov ah,88h
call far [xms]
add edx,512+64
cmp edx,[initial]
jne fail
; An invalid restore must leave both live mappings intact.
push cs
pop es
mov di,mapbuf
ecall 4e00h
mov word [mapbuf+2],2000
mov si,mapbuf
mov ax,4e01h
int 67h
cmp ah,0a3h
jne fail
mov ax,0d000h
mov es,ax
cmp word [es:16],1234h
jne fail
; Check partial save/restore and OS function-set control on the live manager.
push cs
pop es
mov si,partial
mov di,mapbuf
ecall 4f00h
mov bx,1
mov dx,[emhandle]
ecall 4400h
mov si,mapbuf
ecall 4f01h
ecall 5d01h
mov [key],bx
mov [key+2],cx
mov ax,5900h
int 67h
cmp ah,0a4h
jne fail
mov bx,[key]
mov cx,[key+2]
ecall 5d00h
ecall 5d02h
mov dx,[emhandle]
mov si,mapcall
mov [si+2],ds
mov [si+7],ds
mov [si+12],ds
ecall 5600h
mov ax,0d000h
mov es,ax
cmp word [es:16],1234h
jne fail
; Fill every alternate map register set, then change page zero in one bank.
push ds
pop es
mov di,mapbuf
ecall 5900h
mov cx,[mapbuf+2]
cmp cx,7
jne fail
xor si,si
.bank_alloc:
ecall 5b03h
mov [banks+si],bl
inc si
loop .bank_alloc
mov ax,5b03h
int 67h
cmp ah,9bh
jne fail
mov bl,[banks]
ecall 5b01h
mov dx,[emhandle]
mov bx,1
ecall 4400h
mov ax,0d000h
mov es,ax
cmp word [es:16],4567h
jne fail
xor bx,bx
mov es,bx
xor di,di
ecall 5b01h
mov ax,0d000h
mov es,ax
cmp word [es:16],1234h
jne fail
mov cx,7
xor si,si
.bank_free:
mov bl,[banks+si]
ecall 5b04h
inc si
loop .bank_free
mov dx,[emhandle]
ecall 4500h
mov dx,[xhandle]
xcall 0ah
mov ah,88h
call far [xms]
cmp edx,[initial]
jne fail
; Request the entire shared pool through each API and check the other cannot allocate.
mov edx,[initial]
xcall 89h
mov [xhandle],dx
; Persist data at the last word of an allocation, including beyond the 64 MiB boundary.
mov eax,[initial]
shl eax,10
sub eax,2
mov word [copy],0
mov [moveblock+12],eax
mov [readblock+6],eax
mov [moveblock+10],dx
mov [readblock+4],dx
mov si,moveblock
xcall 0bh
mov si,readblock
xcall 0bh
cmp word [copy],05aa5h
jne fail
ecall 4200h
test bx,bx
jnz fail
mov bx,1
mov ax,4300h
int 67h
cmp ah,88h
jne fail
mov dx,[xhandle]
xcall 0ah
mov ah,08h
call far [xms]
mov edx,[initial]
cmp edx,65535
jbe classic
cmp ax,65535
jne fail
jmp max_ems
classic: cmp ax,dx
jne fail
max_ems: ecall 4200h
cmp bx,dx
jne fail
ecall 4300h
mov [emhandle],dx
dec bx
mov [lastpage],bx
ecall 4400h
mov ax,0d000h
mov es,ax
mov word [es:3ffeh],0cafeh
xor bx,bx
ecall 4400h
mov bx,[lastpage]
ecall 4400h
cmp word [es:3ffeh],0cafeh
jne fail
mov ah,88h
call far [xms]
test edx,edx
jnz fail
mov edx,1
mov ah,89h
call far [xms]
cmp bl,0a0h
jne fail
mov dx,[emhandle]
ecall 4500h
mov ah,88h
call far [xms]
cmp edx,[initial]
jne fail
mov dx,ok
jmp report
fail: mov dx,bad
report: mov ah,9
int 21h
mov ax,4c00h
int 21h
ok db 'XMS EMS MEMORY CHECK PASSED',13,10,'$'
bad db 'XMS EMS MEMORY CHECK FAILED',13,10,'$'
mapped:
mov ax,0d000h
mov es,ax
cmp word [es:16],4567h
jne fail
retf
mapcall dw mapped,0
db 1
dw newpages,0
db 1
dw oldpages,0
times 4 dw 0
partial dw 1,0d000h
mapbuf times 26 db 0
banks times 7 db 0
key dd 0
newpages dw 1,0
oldpages dw 0,0
sequence dw 1122h,3344h,5566h
overlap dd 4
dw 0,sequence,0,0,sequence+2,0
xms dd 0
initial dd 0
lastpage dw 0
xhandle dw 0
upper dw 0
emhandle dw 0
value dw 05aa5h
copy dw 0
moveblock dd 2
dw 0,value,0,0
dd 16
readblock dd 2
dw 0
dd 16
dw 0,copy,0
emove dd 2
db 1
dw 0,16,1
db 0
dw 0,copy,0
