#!/usr/bin/env python3
"""Kiểm tra Ctrl-C / Ctrl-Z / Ctrl-\\ / job control qua pty.

    python3 signals.py <jaksh> [home]

Mỗi kịch bản: gõ lệnh, gửi phím, rồi gõ `echo OK-<n>` để chứng minh shell còn
sống và nhận lệnh. In PASS/FAIL từng kịch bản.
"""
import os, pty, re, select, signal, sys, tempfile, time

SHELL = sys.argv[1]
HOME = sys.argv[2] if len(sys.argv) > 2 else tempfile.mkdtemp(prefix="home-")
ANSI = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]|\r")


class Sh:
    def __init__(self):
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.environ.update(HOME=HOME, TERM="xterm-256color")
            os.execv(SHELL, [SHELL])
        self.read(2.5)

    def read(self, t):
        out = b""
        end = time.time() + t
        while time.time() < end:
            r, _, _ = select.select([self.fd], [], [], 0.05)
            if r:
                try:
                    out += os.read(self.fd, 65536)
                except OSError:
                    break
        return ANSI.sub("", out.decode(errors="replace"))

    def send(self, s, wait=1.0):
        os.write(self.fd, s.encode() if isinstance(s, str) else s)
        return self.read(wait)

    def run(self, cmd, wait=1.0):
        return self.send(cmd + "\r", wait)

    def alive(self):
        try:
            return os.waitpid(self.pid, os.WNOHANG)[0] == 0
        except ChildProcessError:
            return False

    def ok(self, n):
        """Shell còn sống và chạy được lệnh mới?"""
        if not self.alive():
            return False
        out = self.run(f"echo OK-{n}-$((0))done".replace("$((0))", ""), 1.0)
        return f"\nOK-{n}-done" in out

    def close(self):
        try:
            os.kill(self.pid, signal.SIGKILL)
            os.waitpid(self.pid, 0)
        except (ProcessLookupError, ChildProcessError):
            pass


def pid_alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


results = []


def check(name, cond, detail=""):
    results.append((name, cond))
    print(f"{'PASS' if cond else 'FAIL'}  {name}" + (f"  — {detail}" if detail and not cond else ""), flush=True)


sh = Sh()

# 1. Ctrl-C lệnh foreground đơn
sh.run("sleep 30", 0.5); sh.send(b"\x03", 0.8)
out = sh.run("echo status=$?", 0.8)
check("1 ^C sleep: $? = 130", "status=130" in out, out[-80:])
check("1 ^C sleep: shell sống", sh.ok(1))

# 2. ^C bỏ phần còn lại của dòng
out = sh.run("sleep 30; echo SHOULD-NOT-RUN", 0.5) + sh.send(b"\x03", 1.0)
check("2 ^C trong `a; b`: không chạy b", "SHOULD-NOT-RUN" not in out.split("sleep 30; echo SHOULD-NOT-RUN")[-1], out[-120:])
out = sh.run("sleep 30 && echo AND-NO || echo OR-NO", 0.5) + sh.send(b"\x03", 1.0)
tail = out.split("echo OR-NO")[-1]
check("2 ^C trong `a && b || c`: không chạy b, c", "AND-NO" not in tail and "OR-NO" not in tail, tail[-120:])
check("2 shell sống", sh.ok(2))

# 3. Pipeline
sh.run("sleep 30 | cat | cat", 0.5); sh.send(b"\x03", 0.8)
check("3 ^C pipeline: shell sống", sh.ok(3))

# 4. Lệnh đọc từ terminal
sh.run("cat", 0.5); sh.send(b"\x03", 0.8)
check("4 ^C `cat` đọc tty: shell sống", sh.ok(4))

# 5. Command substitution
sh.run("echo $(sleep 30)", 0.5); sh.send(b"\x03", 0.8)
check("5 ^C trong $(...): shell sống", sh.ok(5))

# 6. Ctrl-\ (SIGQUIT)
sh.run("sleep 30", 0.5); sh.send(b"\x1c", 0.8)
check("6 Ctrl-\\ sleep: shell sống", sh.ok(6))

# 7. ^C ở prompt trống / dòng đang gõ dở
sh.send(b"\x03", 0.3); sh.send(b"echo half-typed\x03", 0.5)
check("7 ^C ở prompt: shell sống", sh.ok(7))

# 8. Chương trình bỏ qua SIGINT → ^C không làm gì, nó chạy hết
t0 = time.time()
out = sh.run("sh -c 'trap \"\" INT; sleep 1.5; echo survived-trap'", 0.4) + sh.send(b"\x03", 2.0)
check("8 lệnh tự bỏ qua SIGINT vẫn chạy hết", "survived-trap" in out, out[-100:])
check("8 shell sống", sh.ok(8))

# 9. Ctrl-Z → jobs → fg → ^C
out = sh.run("sleep 30", 0.5) + sh.send(b"\x1a", 1.0)
check("9 ^Z: báo đã tạm dừng", "tạm dừng" in out, out[-150:])
check("9 ^Z: shell sống", sh.ok(9))
out = sh.run("jobs", 0.8)
check("9 jobs: Stopped", "Stopped" in out and "sleep 30" in out, out[-150:])
out = sh.run("fg", 0.8)
check("9 fg: in lại lệnh", "sleep 30" in out, out[-100:])
sh.send(b"\x03", 0.8)
check("9 ^C sau fg: shell sống", sh.ok(91))
out = sh.run("jobs", 0.8)
check("9 jobs rỗng sau khi job chết", "sleep 30" not in out, out[-120:])

# 10. Ctrl-Z → bg → kill %1 → thông báo xong
sh.run("sleep 40", 0.5); sh.send(b"\x1a", 1.0)
out = sh.run("bg", 0.8)
check("10 bg: chạy nền", "sleep 40 &" in out, out[-100:])
out = sh.run("jobs -l", 0.8)
check("10 jobs -l: Running", "Running" in out, out[-150:])
m = re.search(r"\[1\]\+\s+(\d+)\s+Running", out)
pgid = int(m.group(1)) if m else 0
sh.run("kill %1", 0.8)
out = sh.run("echo after-kill", 0.8)
check("10 kill %1: báo xong", "xong" in out, out[-150:])
check("10 kill %1: tiến trình thật đã chết", pgid and not pid_alive(pgid), f"pgid={pgid}")

# 11. Job nền không bị ^C của job foreground
out = sh.run("sleep 40 &", 0.8)
m = re.search(r"\[\d+\] (\d+)", out)
bgpid = int(m.group(1)) if m else 0
sh.run("sleep 30", 0.5); sh.send(b"\x03", 0.8)
check("11 job nền sống sót ^C của job khác", bgpid and pid_alive(bgpid), f"bgpid={bgpid}")
sh.run("kill %1", 0.5); sh.run("", 0.5)

# 12. Chương trình để terminal ở raw mode rồi chết → shell khôi phục
sh.run("python3 -c 'import tty; tty.setraw(0)'", 1.0)
out = sh.run("stty -a | tr ' ' '\\n' | grep -x -e icanon -e -icanon -e echo -e -echo", 1.0)
check("12 khôi phục chế độ terminal sau raw mode", "\nicanon" in out and "\necho" in out, out[-100:])

# 13. Pretty curl (in-process capture) — ^C
sh.run("curl -s https://httpbin.org/delay/8", 1.0); sh.send(b"\x03", 1.0)
check("13 ^C khi curl pretty: shell sống", sh.ok(13))

# 14. --jak (chạy qua Command::output trong group của shell) — ^Z bị bỏ qua, không treo
out = sh.run("ls --jak", 1.0)
check("14 ls --jak vẫn chạy", sh.ok(14))

# 15. Thoát khi còn job dừng: cảnh báo, lần 2 mới thoát, job bị dọn
sh.run("sleep 50", 0.5); sh.send(b"\x1a", 1.0)
out = sh.run("jobs -l", 0.8)
m = re.search(r"\[\d+\]\+\s+(\d+)\s+Stopped", out)
spid = int(m.group(1)) if m else 0
out = sh.run("exit", 0.8)
check("15 exit lần 1: cảnh báo, chưa thoát", "tạm dừng" in out and sh.alive(), out[-150:])
sh.run("exit", 1.0)
time.sleep(0.5)
check("15 exit lần 2: thoát", not sh.alive())
check("15 job dừng được dọn (SIGHUP)", spid and not pid_alive(spid), f"spid={spid}")
sh.close()

# 16. Ctrl-D khi không có job → thoát bình thường
sh = Sh()
sh.send(b"\x04", 1.0)
time.sleep(0.3)
check("16 Ctrl-D thoát", not sh.alive())
sh.close()

n = len(results)
ok = sum(c for _, c in results)
print(f"\n{ok}/{n} PASS")
sys.exit(0 if ok == n else 1)
