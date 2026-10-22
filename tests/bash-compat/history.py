#!/usr/bin/env python3
"""Đợt 5 qua pty: kho lịch sử (nhập #V2, --long/--here/--failed, chia sẻ 2 tab),
Ctrl-R fuzzy, xác nhận lệnh nguy hiểm, tô màu cú pháp.

    python3 history.py <jaksh>
"""
import os, pty, re, select, sys, tempfile, time
J = os.path.abspath(sys.argv[1])
HOME = tempfile.mkdtemp(prefix="home-")
os.makedirs(os.path.join(HOME, ".config/jaksh"))
os.makedirs(os.path.join(HOME, "proj"))
with open(os.path.join(HOME, ".config/jaksh/history"), "w") as f:
    f.write("#V2\nold-cmd-from-legacy\ngit push origin main\n")
def spawn():
    pid, fd = pty.fork()
    if pid == 0:
        os.environ.update(HOME=HOME, TERM="xterm-256color"); os.chdir(HOME); os.execv(J, [J])
    return pid, fd
def rd(fd, t):
    out=b""; end=time.time()+t
    while time.time()<end:
        r,_,_=select.select([fd],[],[],0.05)
        if r:
            try: out+=os.read(fd,65536)
            except OSError: break
    return out.decode(errors="replace")
def clean(s): return re.sub(r"\x1b\[[0-9;?]*[A-Za-z]|\r","",s)
def send(fd, s, t=0.7): os.write(fd, s.encode() if isinstance(s,str) else s); return rd(fd, t)
res=[]
a, fa = spawn(); b, fb = spawn(); rd(fa, 2.5); rd(fb, 2.5)
o = clean(send(fa, "history\r")); res.append(("nhập lịch sử cũ (#V2)", "old-cmd-from-legacy" in o))
send(fa, "cd proj && echo in-proj\r"); send(fa, "false\r"); send(fa, "sleep 0.3\r", 1.0)
o = clean(send(fa, "history --long 4\r")); res.append(("history --long có exit code + thời lượng + thư mục", "✗1" in o and "ms" in o and "~/proj" in o or "proj" in o))
o = clean(send(fa, "history --failed\r")); res.append(("history --failed", "false" in o and "in-proj" not in o.split("history --failed")[-1]))
o = clean(send(fa, "history --here\r")); res.append(("history --here (chỉ lệnh ở thư mục hiện tại)", "sleep 0.3" in o and "old-cmd-from-legacy" not in o.split("history --here")[-1]))
send(fb, "echo from-tab-B-xyz\r")
o = clean(send(fa, "history | grep -c from-tab-B\r")); res.append(("lệnh tab B thấy ngay ở tab A (history)", "\n1\n" in o))
# Ctrl-R fuzzy: gõ "tbx" (fuzzy cho from-tab-B-xyz), Enter đưa lên prompt, Enter chạy
o = send(fa, b"\x12", 0.8) + send(fa, "tbx", 0.8)
res.append(("Ctrl-R hiện danh sách fuzzy", "from-tab-B-xyz" in clean(o)))
o = clean(send(fa, b"\r", 0.8) + send(fa, b"\r", 1.0))
res.append(("Ctrl-R Enter → lệnh lên prompt → chạy", "\nfrom-tab-B-xyz" in o))
o = clean(send(fa, b"\x12", 0.6) + send(fa, b"\x1b", 0.6) + send(fa, "echo after-esc\r"))
res.append(("Ctrl-R Esc huỷ, shell vẫn dùng được", "\nafter-esc" in o))
o = clean(send(fa, "rm -rf /\r", 1.0)); res.append(("hỏi xác nhận lệnh nguy hiểm", "Lệnh nguy hiểm" in o and "Vẫn chạy" in o))
o = clean(send(fa, "n\r", 1.0)); res.append(("trả lời n → huỷ", "Đã huỷ" in o))
o = send(fa, "nosuchcmd_q", 0.5); res.append(("tô đỏ lệnh không tồn tại", "\x1b[31mnosuchcmd_q" in o))
send(fa, b"\x15"); o = send(fa, "echo", 0.5); res.append(("tô xanh lệnh có thật", "\x1b[32mecho" in o))
send(fa, b"\x15")
for p in (a, b): os.kill(p, 9)
jl = open(os.path.join(HOME, ".config/jaksh/history.jsonl")).read()
res.append(("file JSONL có cwd/code/ms/sid", '"code":1' in jl and '"cwd"' in jl and '"ms"' in jl))
for n,c in res: print(("PASS" if c else "FAIL")+"  "+n)
ok = sum(c for _, c in res)
print(f"\n{ok}/{len(res)} PASS")
sys.exit(0 if ok == len(res) else 1)
