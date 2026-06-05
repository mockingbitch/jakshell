# JakShell

Shell viết bằng Rust cho macOS & Linux — **nhanh, gọn, thân thiện cho người dùng phổ thông**.

JakShell giữ nguyên cú pháp shell POSIX bạn đã quen, đồng thời bổ sung tiện ích để học và sử dụng terminal dễ hơn: chú thích output, lệnh tự nhiên bằng tiếng Việt, bookmark, workflow git rút gọn, prompt thông minh.

```
JakShell v0.2.0  — shell nhanh, gọn, thân thiện
🌅 Chào buổi sáng, alice!  Thứ Tư, 06/06/2026 · 09:42
💡 Thử `explain ls -la` — JakShell sẽ chú thích từng cột trên output thật.
Gõ ? hoặc help bất cứ lúc nào.

~/code (master *↑1)  ❯
```

---

## Mục lục

- [Tính năng nổi bật](#tính-năng-nổi-bật)
- [Cài đặt](#cài-đặt)
- [Bắt đầu nhanh](#bắt-đầu-nhanh)
- [Lệnh chính](#lệnh-chính)
  - [`explain` — chú thích kết quả lệnh](#explain--chú-thích-kết-quả-lệnh)
  - [`--jak` — tô màu + format output](#--jak--tô-màu--format-output)
  - [`jak <…>` — tiện ích JakShell](#jak--tiện-ích-jakshell)
  - [`jak find` — tìm kiếm tự nhiên](#jak-find--tìm-kiếm-tự-nhiên)
  - [`jak git` — workflow git rút gọn](#jak-git--workflow-git-rút-gọn)
  - [`bookmark` — đặt tên cho lệnh dài](#bookmark--đặt-tên-cho-lệnh-dài)
- [Prompt thông minh](#prompt-thông-minh)
- [Đo thời gian thực thi](#đo-thời-gian-thực-thi)
- [Did-you-mean](#did-you-mean)
- [Cấu hình](#cấu-hình)
- [File & thư mục](#file--thư-mục)
- [Đặt làm shell mặc định](#đặt-làm-shell-mặc-định)
- [Versioning](#versioning)
- [Cấu trúc dự án](#cấu-trúc-dự-án)
- [License](#license)

---

## Tính năng nổi bật

- **Cú pháp POSIX đầy đủ**: `|`, `&&`, `||`, `;`, `&`, `>`, `>>`, `<`, `2>`, `2>>`, `&>`, glob `* ? [abc]`, biến `$VAR ${VAR}`, tilde `~`, quote `'...' "..."`
- **Job nền & quản lý job**: `cmd &`, `jobs`, `fg`, `bg`, `kill`
- **`explain <lệnh>`** — in usage / tham số / ví dụ + **chú thích từng cột giá trị thật** (ls, ps, df, du, free). 70+ lệnh có sẵn chú thích.
- **Cờ `--jak`** trên `ls / ps / df / du / git status / git branch` — tô màu, decode permissions, icon theo loại file.
- **`jak …`** — tiện ích cho người dùng phổ thông: `clean`, `backup`, `update`, `find`, `open`, `sysinfo`, `ip`, `weather`, `theme`, `git …`
- **`bookmark`** — đặt tên cho lệnh dài, chạy qua `jak <name>`.
- **Prompt thông minh trong git repo**: branch, dirty `*`, ahead `↑N`, behind `↓N`, stash `⚑N`, state `MERGE/REBASE/PICK`.
- **Đo thời gian thực thi**: dòng `⏱ X ms` sau mỗi lệnh, kèm exit code khi != 0.
- **Did-you-mean**: gõ sai lệnh → gợi ý lệnh đúng (jaro-winkler).
- **Banner chào hỏi theo giờ**: 🌅 sáng / ☀️ trưa / 🌤 chiều / 🌆 tối / 🌙 đêm khuya + 1 mẹo ngẫu nhiên.
- **Themes dựng sẵn**: `ocean`, `forest`, `sunset`, `mono`, `default`. Đổi nóng bằng `jak theme <tên>`.
- **Tab completion** cho lệnh, alias, builtin, tiểu lệnh `jak …` / `bookmark` / `explain …`, đường dẫn và PATH binary.
- **Cấu hình TOML**: `~/.jakshrc.toml` cho theme / prompt / alias / env / timing / greeting; `~/.jakshrc` script khởi động.

Binary release ~1.4 MB, startup ~10 ms.

---

## Cài đặt

### Yêu cầu
- macOS hoặc Linux
- [Rust](https://rustup.rs) (rustup khuyến nghị) — chỉ cần khi build từ source
- (tuỳ chọn) `git`, `tar`, `curl`, `ripgrep` — cho một số tiện ích `jak …`

### Build và cài đặt

```bash
# 1) Clone (nếu lấy từ git)
git clone https://github.com/mockingbitch/jakshell.git
cd jakshell

# 2) Build + cài đặt
./install.sh
```

`install.sh` sẽ:
1. `cargo build --release`
2. Copy `target/release/jaksh` → `~/.local/bin/jaksh` (đảm bảo `~/.local/bin` ở `PATH`)
3. Tạo `~/.jakshrc.toml` và `~/.jakshrc` mẫu nếu chưa có

### Chạy thử

```bash
~/.local/bin/jaksh
```

### Build thủ công (không dùng install.sh)

```bash
cargo build --release
./target/release/jaksh
```

---

## Bắt đầu nhanh

```bash
# Help
?                           # hoặc: help

# Thông tin máy
jak sysinfo

# Tìm file
jak find Cargo.toml
jak find file "*.rs" in src

# Output đẹp
ls -la --jak
git status --jak
df -h --jak

# Học một lệnh
explain ls -la              # legend + chú thích từng cột thật
explain docker exec         # cú pháp + ví dụ

# Bookmark
bookmark dexec docker exec -it payin_app sh
jak dexec                   # chạy bookmark

# Git rút gọn
jak git save "fix typo"     # add -A && commit -m
jak git sync                # pull --rebase + push
```

---

## Lệnh chính

### `explain` — chú thích kết quả lệnh

```bash
explain                     # liệt kê các lệnh đã có chú thích
explain list                # tương tự
explain ls -la              # legend + live annotate output thật
explain docker exec         # legend (không tự chạy vì interactive)
explain git stash
```

Mỗi `explain` in 4 phần:
1. **Cú pháp** — dạng usage
2. **Tham số / cờ** — bảng cờ thường dùng
3. **Ví dụ** — kèm chú thích từng ví dụ làm gì
4. **Ghi chú** — bí kíp, edge case

Với `ls -l / ps / df / du / free`: ngoài legend còn **chú thích từng giá trị trên output thật** (decode permissions, map header với row đầu, v.v.).

**Đã có chú thích cho 70+ lệnh**, bao trùm:
- Điều hướng: `cd`, `pwd`, `ls`, `find`
- File: `cp`, `mv`, `rm`, `mkdir`, `rmdir`, `touch`, `ln`, `chmod`, `chown`
- Xem: `cat`, `less`, `head`, `tail`, `echo`
- Lọc: `grep`, `sort`, `uniq`, `wc`, `cut`, `tr`, `xargs`
- Process: `ps`, `top`, `kill`, `pkill`, `killall`
- Đĩa: `df`, `du`, `free`, `stat`, `lsof`
- Mạng: `ssh`, `ssh-keygen`, `ssh-copy-id`, `ssh-add`, `sftp`, `scp`, `curl`, `wget`, `ping`, `netstat`, `ss`, `ifconfig`, `ip`
- Nén: `tar`, `zip`, `unzip`
- Hệ thống: `uptime`, `who`, `date`, `env`, `alias`, `history`, `which`, `man`
- Git: `git`, `status`, `log`, `diff`, `branch`, `clone`, `init`, `add`, `commit`, `push`, `pull`, `fetch`, `merge`, `rebase`, `reset`, `restore`, `revert`, `stash`, `tag`, `remote`, `checkout`, `switch`, `cherry-pick`, `blame`, `show`, `reflog`, `config`
- Docker: `docker`, `ps`, `exec`, `run`, `build`, `images`, `pull`, `push`, `logs`, `stop/start/restart/kill`, `rm`, `rmi`, `inspect`, `network`, `volume`, `compose`, `cp`, `login`, `system`, `tag`

> Lệnh destructive (`rm`, `mv`, `chmod`, `kill`, `docker exec`, …) chỉ in legend, KHÔNG tự chạy để tránh thao tác nhầm.

---

### `--jak` — tô màu + format output

Thêm cờ `--jak` vào lệnh được hỗ trợ để JakShell intercept output và render đẹp hơn. Lệnh thật **không thấy** `--jak` (bị tách ra trước).

```bash
ls -la --jak                # tô màu permissions từng ký tự, icon dir/exec
ps aux --jak                # PID cyan, %CPU/%MEM theo ngưỡng (xanh/vàng/đỏ)
df -h --jak                 # Use% xanh→vàng→đỏ; size màu theo K/M/G/T
du -sh --jak                # căn cột size
git status --jak            # bố cục lại theo section với icon ✎/+/✗/⚠
git branch --jak            # ● branch hiện tại (bold green)
```

Lưu ý:
- Chỉ áp dụng cho **single command**, không pipe / không background.
- Tự tôn trọng `theme.use_color` — theme `mono` không có ANSI.

---

### `jak <…>` — tiện ích JakShell

```bash
jak help                    # hoặc: jak ?
```

| Lệnh | Mô tả |
|------|-------|
| `jak clean [--dry]` | Xoá file tạm & cache trong `~/.cache`, `/tmp` (file bạn sở hữu) |
| `jak backup <thư_mục>` | Nén thành `.tar.gz` với tên `<thư_mục>-YYYYMMDD-HHMMSS.tar.gz` |
| `jak update` | Tự dò brew / port / apt / dnf / pacman / zypper / apk và chạy update + upgrade |
| `jak find …` | Tìm file/thư mục/nội dung (xem mục [jak find](#jak-find--tìm-kiếm-tự-nhiên)) |
| `jak open <path>` | Mở bằng app mặc định (`open` trên macOS, `xdg-open` trên Linux) |
| `jak sysinfo` | OS, CPU, RAM, đĩa |
| `jak ip` | IP nội bộ + public (thử ipconfig/hostname/ip, curl/wget) |
| `jak weather [tp]` | Thời tiết qua wttr.in |
| `jak theme <tên>` | Đổi giao diện nóng: `ocean / forest / sunset / mono / default` |
| `jak git …` | Workflow git rút gọn (xem [jak git](#jak-git--workflow-git-rút-gọn)) |
| `jak <bookmark>` | Chạy bookmark đã đặt (xem [bookmark](#bookmark--đặt-tên-cho-lệnh-dài)) |

Mọi `jak …` đều **defensive check** — báo lỗi tiếng Việt rõ ràng nếu thiếu dependency (vd `tar`, `curl`, `xdg-open`).

---

### `jak find` — tìm kiếm tự nhiên

```bash
jak find help

jak find <tên>                              # = jak find file <tên>
jak find file <tên> [in <path>]
jak find dir  <tên> [in <path>]
jak find text "<chuỗi>" [in <path>]         # dùng ripgrep nếu có, fallback grep
jak find big  [in <path>]                   # 20 file lớn nhất
jak find recent [in <path>]                 # file sửa trong 24h
jak find empty [in <path>]                  # file rỗng
```

Quy tắc:
- Pattern có `* ? [abc]` → **glob match**; ngược lại → **substring không phân biệt hoa thường**.
- Từ khoá `in` hoặc `trong` đều được; đường dẫn hỗ trợ `~`.
- Tự bỏ qua: `.git`, `node_modules`, `target`, `venv`, `__pycache__`, `dist`, `build`, `.idea`, `.vscode`, `.gradle`, `.next`, `.nuxt`, `.venv`.
- Quote tôn trọng chuẩn POSIX: `"*.rs"` là literal, `*.rs` (không nháy) bị shell glob-expand trước.

Lệnh `find` POSIX gốc (`find . -name "*.rs"`) **vẫn dùng như bình thường** — JakShell không can thiệp.

---

### `jak git` — workflow git rút gọn

```bash
jak git help
```

| Shortcut | Tương đương |
|----------|-------------|
| `jak git save "<msg>"` | `git add -A && git commit -m "<msg>"` |
| `jak git wip` | `git add -A && git commit -m "WIP"` |
| `jak git sync` | `git pull --rebase && git push` |
| `jak git publish [<branch>]` | `git push -u origin <branch>` (lần đầu push) |
| `jak git amend` | `git commit --amend --no-edit` |
| `jak git uncommit` | `git reset --soft HEAD~1` (huỷ commit cuối, giữ staged) |
| `jak git undo` | `git restore --staged .` |
| `jak git unstage <file>` | `git restore --staged <file>` |
| `jak git clean-branches` | Xoá branch local đã merged (có xác nhận) |

Mỗi bước in `$ git …` trước khi chạy — bạn luôn biết shortcut đang làm gì.

---

### `bookmark` — đặt tên cho lệnh dài

```bash
bookmark                                       # liệt kê
bookmark help

# Tạo / cập nhật
bookmark docker_app docker exec -it payin_app sh
bookmark deploy ./scripts/deploy.sh prod
bookmark add greet echo "xin chào"             # cú pháp rõ ràng hơn

# Chạy
jak docker_app                                 # → docker exec -it payin_app sh
jak docker_app -e ENV=prod                     # tham số nối vào cuối

# Quản lý
bookmark show docker_app
bookmark del docker_app                        # = rm / remove
```

Lưu tại `~/.config/jaksh/bookmarks.toml` — sửa tay được:

```toml
docker_app = "docker exec -it payin_app sh"
deploy     = "./scripts/deploy.sh prod"
```

Bookmark sẽ hiện trong `jak help` để dễ nhớ. Hỗ trợ cú pháp shell đầy đủ (pipe, redirect, biến) vì được đẩy qua lexer/parser/executor như gõ tay.

---

## Prompt thông minh

Trong git repo, prompt sẽ hiện thêm thông tin sau tên branch:

```
~/code  master *↑2↓1 ⚑3 MERGE  ❯
```

| Ký hiệu | Ý nghĩa |
|---------|---------|
| `<branch>` | branch hiện tại (cyan); detached HEAD → màu magenta |
| `*` | dirty — có staged / modified / untracked |
| `⚠N` | N file conflict |
| `↑N` | đi trước upstream N commit |
| `↓N` | đi sau upstream N commit |
| `⚑N` | có N stash |
| `MERGE / REBASE / PICK / REVERT / BISECT` | đang dở dang |

Tốc độ: 1 lần gọi `git status --branch --porcelain=v2`, state + stash đọc từ file `.git/`.

Mã thoát hiển thị qua màu mũi tên: xanh = OK, đỏ = lỗi.

---

## Đo thời gian thực thi

Sau mỗi lệnh, JakShell in dòng `⏱` dim:

```
⏱  25 µs                                # builtin
⏱  506 ms                               # sleep 0.5
⏱  12 ms                                # spawn external
⏱  1 ms   ✗ exit 127                    # lệnh fail (đỏ)
⏱  2m 15.4s                             # build dài
```

Format tự đổi đơn vị: `µs / ms / s / m+s / h+m+s`.

Cấu hình trong `[timing]`:
```toml
[timing]
enabled = true
threshold_ms = 0          # 0 = luôn hiện; đặt 500 nếu thấy ồn
show_status = true        # in "✗ exit N" đỏ khi exit code != 0
```

---

## Did-you-mean

Gõ sai lệnh → JakShell gợi ý:

```
$ gitt status
jaksh: không tìm thấy lệnh: gitt
💡 có phải bạn muốn: git?
```

Dùng thuật toán jaro-winkler, ngưỡng 0.86. Bỏ qua các namespace nội bộ (`jak`, `explain`) để không gợi ý nhảm.

---

## Cấu hình

### `~/.jakshrc.toml` — cấu hình TOML

```toml
# Prompt template. Biến: {accent} {success} {error} {dim} {reset}
# {cwd} {cwd_short} {git} {arrow} {status}
prompt = "{accent}{cwd_short}{reset}{git} {arrow} "

[theme]
accent = "bright_cyan"
success = "bright_green"
error = "bright_red"
dim = "bright_black"
arrow = "❯"
git_branch_icon = " "
use_color = true

[timing]
enabled = true
threshold_ms = 0
show_status = true

[greeting]
enabled = true
show_greeting = true        # dòng "Chào buổi …"
show_tip = true             # mẹo ngẫu nhiên
name = ""                   # rỗng = lấy $USER

[aliases]
ll = "ls -lah"
gs = "git status"
gd = "git diff"
gl = "git log --oneline --graph --decorate -20"

# Alias tiếng Việt cho lowtech
tim = "jak find"
dondep = "jak clean"
napnhat = "jak update"
mo = "jak open"
maytinh = "jak sysinfo"
giaithich = "explain"

[env]
EDITOR = "vim"
LANG = "en_US.UTF-8"
```

### `~/.jakshrc` — script khởi động (POSIX-like)

Chạy SAU khi đọc `.jakshrc.toml`. Mỗi dòng là một lệnh shell hợp lệ:

```bash
export PATH=$HOME/.local/bin:$PATH
alias c=clear
alias ..=cd ..
```

---

## File & thư mục

| Đường dẫn | Mục đích |
|-----------|----------|
| `~/.jakshrc.toml` | Cấu hình chính (theme, prompt, alias, env, timing, greeting) |
| `~/.jakshrc` | Script khởi động (chạy mỗi khi mở shell) |
| `~/.config/jaksh/history` | Lịch sử lệnh |
| `~/.config/jaksh/bookmarks.toml` | Bookmark do `bookmark` quản lý |

---

## Đặt làm shell mặc định

```bash
# Đăng ký JakShell với hệ thống
echo "$HOME/.local/bin/jaksh" | sudo tee -a /etc/shells

# Đổi shell mặc định cho user hiện tại
chsh -s "$HOME/.local/bin/jaksh"
```

Mở terminal mới — JakShell sẽ là shell login.

> Trên macOS: nếu `chsh` báo `non-standard shell`, kiểm tra `/etc/shells` đã có đường dẫn `jaksh` chưa.

---

## Versioning

Phiên bản hiển thị trong banner & `jak help` được sinh ở **build time** từ git:

```
git describe --tags --always --dirty=-dirty
```

Quy tắc:
- Có tag: `v0.2.0` → hiện `v0.2.0`
- Có tag + commit thêm: `v0.2.0-3-g91b4d81` (3 commit sau tag v0.2.0)
- Working tree dirty: `v0.2.0-3-g91b4d81-dirty`
- Repo chưa có tag: short SHA, vd `91b4d81`
- Không có git: fallback `Cargo.toml`'s `version`

Để release version mới:
```bash
git tag -a v0.2.0 -m "Release 0.2.0"
cargo build --release
./target/release/jaksh   # banner sẽ hiện v0.2.0
```

---

## Cấu trúc dự án

```
JakShell/
├── Cargo.toml           # package + dependencies
├── build.rs             # nhúng version từ git vào binary
├── install.sh           # build + cài đặt vào ~/.local/bin
├── README.md
├── LICENSE
├── examples/
│   ├── jakshrc.toml     # cấu hình mẫu (TOML)
│   └── jakshrc          # script khởi động mẫu
└── src/
    ├── main.rs          # REPL, welcome banner, timing display
    ├── shell.rs         # Shell state, env, jobs, configs
    ├── lexer.rs         # Tokenizer
    ├── parser.rs        # AST (pipeline, redirect, seq)
    ├── expand.rs        # Variable / tilde / glob expansion
    ├── executor.rs      # Run AST: pipes, redirects, background
    ├── builtins/mod.rs  # cd, alias, export, history, explain, bookmark…
    ├── prompt.rs        # Prompt + smart git segment
    ├── theme.rs         # Colors + themes
    ├── config.rs        # ~/.jakshrc.toml + ~/.jakshrc loader
    ├── completion.rs    # Tab completion (rustyline helper)
    ├── history.rs       # (handled by rustyline)
    ├── suggest.rs       # Did-you-mean
    ├── explain.rs       # 70+ command explanations + live annotators
    ├── pretty.rs        # --jak prettifier (ls/ps/df/du/git)
    ├── jak.rs           # jak <subcommand> router + utilities
    ├── findcmd.rs       # `jak find` backend (file/dir/text/big/recent/empty)
    └── bookmark.rs      # bookmark builtin + storage
```

---

## License

MIT
