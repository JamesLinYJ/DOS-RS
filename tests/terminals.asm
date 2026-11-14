; Independent 8086 processes for the virtual terminal regression.
bits 16
org 100h
%macro private_frame 1
mov bx,1
mov ax,4300h
int 67h
test ah,ah
jnz fail
xor bx,bx
mov ax,4400h
int 67h
test ah,ah
jnz fail
mov ax,0d000h
mov es,ax
mov word [es:16],%1
%endmacro
%ifdef SPIN
private_frame 0a551h
mov dx, ready
call print
again:
mov ax, 3d00h
mov dx, path
int 21h
jc again
mov bx, ax
mov ah, 3eh
int 21h
cmp word [es:16],0a551h
jne fail
mov dx, seen
call print
jmp $
ready db 'BACKGROUND RUNNING',13,10,'$'
seen db 'BACKGROUND SAW FILE',13,10,'$'
path db 'SHARED.TXT',0
%elifdef WAITKEY
mov dx, ready
call print
xor ax,ax
int 16h
cmp al,'a'
jne fail
mov dx, woke
call print
mov ax,4c00h
int 21h
ready db 'WAITING FOR A',13,10,'$'
woke db 'KEY DELIVERED',13,10,'$'
%elifdef CRASH
db 63h
%else
private_frame 05aa2h
push dx
mov ax,3c00h
xor cx,cx
mov dx,path
int 21h
jc fail
mov bx,ax
mov dx,data
mov cx,length
mov ah,40h
int 21h
jc fail
mov ah,3eh
int 21h
jc fail
cmp word [es:16],05aa2h
jne fail
pop dx
mov ax,4500h
int 67h
test ah,ah
jnz fail
mov dx,done
call print
mov ax,4c00h
int 21h
path db 'SHARED.TXT',0
data db 'VT shared file'
length equ $-data
done db 'CALCULATION COMPLETE',13,10,'$'
%endif
fail: mov ax,4c01h
int 21h
print: mov ah,9
int 21h
ret
