"""jaksh chạy bên trong bash: ^C/^Z trong jaksh, thoát ra bash vẫn dùng được."""
import os, pty, re, select, sys, time, signal, tempfile
J = sys.argv[1]; HOME = tempfile.mkdtemp()
pid, fd = pty.fork()
if pid == 0:
    os.environ.update(HOME=HOME, TERM="xterm-256color", PS1="BASH$ ")
    os.execv("/bin/bash", ["bash", "--norc", "-i"])
def rd(t):
    out=b""; end=time.time()+t
    while time.time()<end:
        r,_,_=select.select([fd],[],[],0.05)
        if r:
            try: out+=os.read(fd,65536)
            except OSError: break
    return re.sub(r"\x1b\[[0-9;?]*[A-Za-z]|\r","",out.decode(errors="replace"))
def run(c, t=1.0): os.write(fd, (c+"\r").encode()); return rd(t)
rd(1)
res=[]
run(J, 2.5)
run("sleep 30", 0.5); os.write(fd, b"\x03"); rd(0.8)
o = run("echo IN-JAKSH-OK"); res.append(("^C trong jaksh lồng trong bash", "\nIN-JAKSH-OK" in o))
run("sleep 30", 0.5); os.write(fd, b"\x1a"); rd(0.8)
o = run("jobs"); res.append(("^Z trong jaksh lồng: job Stopped", "Stopped" in o))
run("kill %1", 0.5)
o = run("exit", 1.5)
o = run("echo BACK-IN-BASH"); res.append(("thoát jaksh → bash nhận lại terminal", "\nBACK-IN-BASH" in o))
o = run("sleep 30", 0.5); os.write(fd, b"\x03"); o = run("echo BASH-STILL-OK"); res.append(("bash vẫn xử lý ^C bình thường", "\nBASH-STILL-OK" in o))
# jaksh chạy nền từ bash → tự dừng, không tranh terminal
o = run(J + " &", 1.5)
o = run("jobs"); res.append(("`jaksh &` tự dừng (SIGTTIN), không tranh terminal", "Stopped" in o))
o = run("echo BASH-OK-2"); res.append(("bash vẫn dùng được khi jaksh nền", "\nBASH-OK-2" in o))
o = run("fg", 2.5); o = run("echo FG-JAKSH"); res.append(("fg jaksh từ bash → jaksh chạy", "\nFG-JAKSH" in o))
run("exit", 1.0); o = run("echo BASH-OK-3"); res.append(("thoát lần 2 → về bash", "\nBASH-OK-3" in o))
os.kill(pid, signal.SIGKILL)
for n,c in res: print(("PASS" if c else "FAIL")+"  "+n)
