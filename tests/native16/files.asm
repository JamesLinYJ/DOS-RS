; Native DOS API regression. Run from either A: or C:.
cpu 8086
bits 16
org 100h
mov ax,3000h
stc
int 21h
jnc bad
cmp ax,0a07h
jne bad
mov ax,3306h
int 21h
cmp bx,0a07h
jne bad
or dx,dx
jnz bad
mov dx,source
mov ax,3d00h
int 21h
jc bad
mov bx,ax
mov ax,4200h
mov cx,1
mov dx,2
int 21h
jc bad
cmp dx,1
jne bad
cmp ax,2
jne bad
mov ah,3fh
mov cx,4
mov dx,buffer
int 21h
jc bad
cmp ax,4
jne bad
cmp word [buffer],4241h
jne bad
cmp word [buffer+2],4443h
jne bad
mov ah,3eh
int 21h
jc bad
mov ah,3ch
xor cx,cx
mov dx,dest
int 21h
jc bad
mov bx,ax
mov ah,40h
mov cx,4
mov dx,buffer
int 21h
jc bad
cmp ax,4
jne bad
mov ah,3eh
int 21h
jc bad
mov dx,success
jmp report
bad: mov dx,error
report: mov ah,9
int 21h
mov ax,4c00h
int 21h
source db 'C:\BIG.BIN',0
dest db 'C:\API.TXT',0
success db 'INT21 7.10 FILE API OK',13,10,'$'
error db 'INT21 FILE API FAILED',13,10,'$'
buffer times 4 db 0
