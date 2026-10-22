#!/usr/bin/env python3
"""Kiểm tra REPL qua pty: trap INT, Ctrl-X Ctrl-E, fc -s, history -d, Tab $BIẾN,
set -o vi, pushd/popd.

    python3 repl.py <jaksh>
"""
import os, pty, re, select, sys, tempfile, time
J = os.path.abspath(sys.argv[1])
HOME = tempfile.mkdtemp(prefix="home-")
# "Editor" giả: sửa ORIGINAL → EDITED-BY-EDITOR trong file được mở.
ED = os.path.join(HOME, "fake-editor.sh")
with open(ED, "w") as f:
    f.write('#!/bin/sh\nsed -i "s/ORIGINAL/EDITED-BY-EDITOR/" "$1"\n')
os.chmod(ED, 0o755)
pid, fd = pty.fork()
if pid == 0:
    os.environ.update(HOME=HOME, TERM="xterm-256color", EDITOR=ED); os.execv(J, [J])
def rd(t):
    out=b""; end=time.time()+t
    while time.time()<end:
        r,_,_=select.select([fd],[],[],0.05)
        if r:
            try: out+=os.read(fd,65536)
            except OSError: break
    return re.sub(r"\x1b\[[0-9;?]*[A-Za-z]|\r","",out.decode(errors="replace"))
def send(s,t=0.7): os.write(fd,s.encode() if isinstance(s,str) else s); return rd(t)
res=[]
rd(2.5)
send("trap 'echo TRAPPED-INT' INT\r")
send("sleep 20\r", 0.6); o = send(b"\x03", 1.2) + send("echo alive\r")
res.append(("trap INT chạy khi Ctrl-C lệnh foreground", "TRAPPED-INT" in o and "\nalive" in o))
send("trap - INT\r")
o = send("echo ORIGINAL") + send(b"\x18\x05", 2.0)
res.append(("Ctrl-X Ctrl-E: sửa trong editor rồi chạy", "\nEDITED-BY-EDITOR" in o))
send("echo first-cmd\r")
o = send("fc -s first=again\r", 1.0); res.append(("fc -s cũ=mới chạy lại", "\nagain-cmd" in o))
send("echo marker-delete-me\r")
o = send("history | grep -c marker-delete\r"); n_before = o
send("history -d -3\r")
o = send("history | grep -c marker-delete-me\r"); res.append(("history -d xoá entry", "\n1\n" in o))
o = send("echo $HOS\t", 0.6) + send("\t", 1.0); res.append(("Tab hoàn thành $BIẾN", "HOSTNAME" in o or "HOSTTYPE" in o))
send(b"\x15")
send("set -o vi\r")
o = send("echo vi-mode-xyz", 0.3) + send(b"\x1b", 0.3) + send("0", 0.3) + send("x", 0.3) + send("\r", 0.8)
res.append(("set -o vi: Esc 0 x xoá ký tự đầu", "\ncho vi-mode-xyz" in o or "cho: " in o or "không tìm thấy lệnh: cho" in o))
send("set -o emacs\r")
o = send("cd /tmp && pushd / && popd\r", 1.0); res.append(("pushd/popd in ngăn xếp", "/ /tmp" in o))
os.kill(pid, 9)
for n,c in res: print(("PASS" if c else "FAIL")+"  "+n)
ok = sum(c for _, c in res)
print(f"\n{ok}/{len(res)} PASS")
sys.exit(0 if ok == len(res) else 1)
