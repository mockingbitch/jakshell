#!/usr/bin/env python3
"""So sánh bash vs jaksh trên các case trong cases.py.

Cách chạy:
    python3 run.py /path/to/jaksh            # bảng tóm tắt
    python3 run.py /path/to/jaksh --json out.json
    python3 run.py /path/to/jaksh --only if-basic,for-list -v

Mỗi case chạy trong thư mục tạm mới với HOME tạm (không đụng ~/.config/jaksh
thật). So sánh stdout + exit code; stderr chỉ so có/không (thông điệp lỗi của
hai shell khác nhau là chuyện bình thường).
"""
import json, os, re, shutil, subprocess, sys, tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from cases import CASES

ANSI = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]")


def setup(d):
    for name, body in [("a.txt", "1\n2\n3\n"), ("b.txt", "b\n"), ("c.log", "c\n")]:
        with open(os.path.join(d, name), "w") as f:
            f.write(body)
    os.makedirs(os.path.join(d, "sub", "deep"))
    open(os.path.join(d, "sub", "deep", "x.md"), "w").close()
    open(os.path.join(d, ".hidden"), "w").close()


def run(shell, code, home):
    d = tempfile.mkdtemp(prefix="case-")
    setup(d)
    env = {"HOME": home, "PATH": os.environ["PATH"], "USER": os.environ.get("USER", "u"),
           "LANG": "C.UTF-8", "TERM": "dumb"}
    try:
        p = subprocess.run([shell, "-c", code], cwd=d, env=env, capture_output=True,
                           timeout=5, stdin=subprocess.DEVNULL)
        out, err, rc = p.stdout.decode(errors="replace"), p.stderr.decode(errors="replace"), p.returncode
    except subprocess.TimeoutExpired:
        out, err, rc = "", "TIMEOUT", -999
    finally:
        shutil.rmtree(d, ignore_errors=True)
    return ANSI.sub("", out), ANSI.sub("", err), rc


def main():
    jaksh = sys.argv[1]
    only = None
    if "--only" in sys.argv:
        only = set(sys.argv[sys.argv.index("--only") + 1].split(","))
    verbose = "-v" in sys.argv
    home = tempfile.mkdtemp(prefix="home-")
    results = []
    for cid, group, code in CASES:
        if only and cid not in only:
            continue
        b = run("bash", code, home)
        j = run(jaksh, code, home)
        same = b[0] == j[0] and b[2] == j[2]
        results.append({"id": cid, "group": group, "code": code, "pass": same,
                        "bash": {"out": b[0], "err": b[1], "rc": b[2]},
                        "jaksh": {"out": j[0], "err": j[1], "rc": j[2]}})
        mark = "PASS" if same else "FAIL"
        print(f"{mark}  {group:9} {cid}")
        if verbose or not same:
            if not same:
                print(f"      bash : rc={b[2]} out={b[0]!r} err={b[1][:120]!r}")
                print(f"      jaksh: rc={j[2]} out={j[0]!r} err={j[1][:160]!r}")
    n = len(results)
    ok = sum(r["pass"] for r in results)
    print(f"\n{ok}/{n} khớp bash")
    if "--json" in sys.argv:
        with open(sys.argv[sys.argv.index("--json") + 1], "w") as f:
            json.dump(results, f, ensure_ascii=False, indent=1)
    shutil.rmtree(home, ignore_errors=True)


if __name__ == "__main__":
    main()
