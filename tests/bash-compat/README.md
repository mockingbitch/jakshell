# Bộ test tương thích bash

So sánh hành vi của `jaksh` với `bash`. Mọi kịch bản chạy với `HOME` tạm, nên không đụng tới `~/.config/jaksh` thật.

| File | Nội dung |
|---|---|
| `cases.py` + `run.py` | 215 đoạn lệnh (160 cơ bản + 55 script nâng cao) chạy bằng `bash -c` và `jaksh -c`, so stdout + exit code |
| `signals.py` | Ctrl-C / Ctrl-Z / Ctrl-\ / `fg` / `bg` / `kill %N` / thoát khi còn job dừng, lái qua pty |
| `nested.py` | jaksh chạy bên trong bash: trả terminal đúng khi thoát, `jaksh &` không tranh terminal |

```bash
cargo build
python3 tests/bash-compat/run.py target/debug/jaksh              # tóm tắt + diff case lỗi
python3 tests/bash-compat/run.py target/debug/jaksh --only if-basic,heredoc -v
python3 tests/bash-compat/signals.py target/debug/jaksh          # phải 100% PASS
python3 tests/bash-compat/nested.py target/debug/jaksh
```

Khi sửa một tính năng trong [docs/bash-gap-audit.md](../../docs/bash-gap-audit.md), case tương ứng trong `cases.py` phải chuyển sang PASS. `signals.py` và `nested.py` không được có case FAIL nào.
