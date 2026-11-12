; 8086 request-packet probes. Assemble with CHARACTER or BLOCK for a .SYS,
; or without either define for the DOS client. No native port access here.
bits 16
cpu 8086
%ifdef CHARACTER
%define DRIVER 1
%endif
%ifdef BLOCK
%define DRIVER 1
%endif
%ifdef DRIVER
org 0
%ifdef CHAIN
dw second_header,0
%else
dw -1,-1
%endif
%ifdef CHARACTER
dw 8000h, strategy, interrupt
db 'ECHOBOX '
%else
dw 0, strategy, interrupt
db 1,0,0,0,0,0,0,0
%endif
%ifdef CHAIN
second_header dw -1,-1,8000h,strategy,interrupt
db 'ECHOALT '
%endif
packet dd 0
strategy:
    mov [cs:packet],bx
    mov [cs:packet+2],es
    retf
interrupt:
    push cs
    pop ds
    les bx,[packet]
    mov al,[es:bx+2]
    or al,al
    jz .init
    cmp al,4
    je .read
    cmp al,8
    jne .bad
    lds si,[es:bx+14]
%ifdef BLOCK
    push cs
    pop es
    mov di,saved
    mov cx,256
    cld
    rep movsw
%else
    mov al,[si]
    mov [cs:saved],al
%endif
    jmp .done
.read:
    les di,[es:bx+14]
%ifdef BLOCK
    mov si,saved
    mov cx,256
    cld
    rep movsw
%else
    mov al,[cs:saved]
    mov [es:di],al
%endif
    jmp .done
.init:
%ifdef REJECT
    jmp .bad
%endif
    mov word [es:bx+14],resident_end
    mov [es:bx+16],cs
%ifdef BLOCK
    mov byte [es:bx+13],1
%endif
.done:
    les bx,[cs:packet]
    mov word [es:bx+3],100h
    retf
.bad:
    mov word [es:bx+3],8103h
    retf
saved db 0
%ifdef BLOCK
    times 511 db 0
%endif
resident_end:
%else
org 100h
start:
%ifdef SETVER
    mov ah,52h
    int 21h
    les di,[es:bx+37h]
    cmp word [es:di-6],4453h
    jne bad
    cmp word [es:di-4],5249h
    jne bad
    push ds
    pop es
%endif
    ; Raw C: must never resolve a create against the mounted A: filesystem.
    mov dx,raw_path
    xor cx,cx
    mov ah,3ch
    int 21h
    jnc bad
    cmp ax,15
    jne bad
    mov dx,alt_name
    mov ax,3d00h
    int 21h
    jc bad
    mov bx,ax
    mov ah,3eh
    int 21h
    jc bad
    mov dx,echo_name
    mov ax,3d02h
    int 21h
    jc bad
    mov bx,ax
    mov ax,4400h
    int 21h
    jc bad
    test dl,80h
    jz bad
    mov ah,40h
    mov cx,1
    mov dx,byte_out
    int 21h
    jc bad
    cmp ax,1
    jne bad
    mov ah,3fh
    mov dx,buffer
    int 21h
    jc bad
    cmp ax,1
    jne bad
    cmp byte [buffer],'Q'
    jne bad
    mov ah,3eh
    int 21h
    jc bad
    mov ax,2
    mov bx,byte_out
    mov cx,1
    xor dx,dx
    int 26h
    pop si
    jc bad
    mov ax,2
    mov bx,buffer
    mov byte [buffer],0
    int 25h
    pop si
    jc bad
    cmp byte [buffer],'Q'
    jne bad
    cmp byte [buffer+511],7ah
    jne bad
    mov dx,80h
    mov bx,55aah
    mov ah,41h
    int 13h
    jc bad
    cmp bx,0aa55h
    jne bad
    mov [dap+6],ds
    mov si,dap
    mov ah,42h
    int 13h
    jc bad
    cmp word [buffer],4449h
    jne bad
    cmp byte [buffer+511],0a5h
    jne bad
    mov dx,msg
    mov ah,9
    int 21h
    ; COM2 and LPT1 must reach distinct host files.
    mov dx,1
    mov ax,00e3h
    int 14h
    mov ax,0153h
    int 14h
    test ah,80h
    jnz bad
    xor dx,dx
    mov ax,0050h
    int 17h
    test ah,1
    jnz bad
    ; Exercise interpreted OUT DX,AL against the same second UART.
    mov dx,2f8h
    mov al,'D'
    out dx,al
    mov dx,keyboard
    mov ah,9
    int 21h
    mov ah,0
    int 16h
    cmp ax,1e61h
    jne bad
    mov dx,success
    mov ah,9
    int 21h
    mov ax,4c00h
    int 21h
bad:
    mov dx,failure
    mov ah,9
    int 21h
    mov ax,4c01h
    int 21h
raw_path db 'C:\NOFILE.DAT',0
alt_name db 'ECHOALT',0
echo_name db 'ECHOBOX',0
byte_out db 'Q'
         times 510 db 0
         db 7ah
buffer   times 512 db 0
dap db 16,0
    dw 1,buffer,0
    dq 7
msg db 'IDE LBA 7 OK',13,10,'CHAR AND BLOCK I/O OK',13,10,'$'
keyboard db 'WAITING FOR PS2',13,10,'$'
success db 'DEVICE PROBE OK',13,10,'$'
failure db 'DEVICE PROBE FAILED',13,10,'$'
%endif
