# Changelog

Tất cả thay đổi đáng kể của JakShell được ghi lại tại đây.

Format theo [Keep a Changelog](https://keepachangelog.com/vi/1.1.0/),
tuân thủ [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [v1.0.4] — Paste nhiều dòng + docker completion + format curl

### Đã thêm

- **Tab completion cho docker/podman** — `docker exec -it <Tab>` gợi ý container đang chạy (`docker ps`); `start/rm/inspect` gợi ý mọi container kể cả đã dừng (`docker ps -a`); `run/rmi/tag` gợi ý image. Subcommand nhận nhiều container (`stop a b c`) gợi ý ở mọi vị trí. Đang gõ flag (`-it`) thì không gợi ý. Icon `🐳` phân biệt container/image. Docker không cài / không có container → rơi về path completion như cũ.
- **Auto-format response của `curl`** — chạy `curl` tương tác sẽ thấy đường ngăn cách `── HTTP 200 · application/json · 233 ms ──` tách phần lệnh với phần response (status xanh/vàng/đỏ theo 2xx/3xx/4xx-5xx), body JSON được indent + tô màu (key cyan, string xanh, số vàng, bool/null magenta), giữ nguyên thứ tự field của API. Body không phải JSON in nguyên si. Tự inject `-sS` để tắt progress meter nhưng vẫn hiện lỗi.
  - Chỉ bật khi stdout là TTY, không pipe / redirect / chạy nền — `curl | jq`, `curl > file`, script/CI vẫn nhận raw output, không vỡ gì.

### Đã sửa

- **Paste nhiều dòng bị gộp thành 1 lệnh** — lexer coi `\n` là whitespace nên paste `cd /tmp` + `ls` chạy thành `cd /tmp ls`. Giờ xuống dòng = dấu phân tách lệnh (như `;`); newline trong nháy đơn/kép vẫn giữ nguyên.
- **`\` cuối dòng (line continuation)** — lệnh nhiều dòng kiểu `curl ... \` từng vỡ thành rác `\n--header` khiến curl báo "Malformed input to a URL". Giờ `\` + xuống dòng = nối dòng (cả ngoài nháy lẫn trong nháy kép), giống bash.
- Comment `#` chỉ bỏ qua tới hết dòng — không còn nuốt các dòng phía sau khi paste nhiều dòng.
- Dòng trống / `;` thừa / `;;` không còn báo lỗi "lệnh trống".
- Paste nhiều dòng lưu **mỗi dòng thành 1 entry lịch sử riêng** (gọn cho `↑` / `Ctrl-R`) thay vì 1 khối có `\n` ở giữa.

---

## [v1.0.3] — Tương thích VSCode + markdown render

### Đã thêm

- **CLI args bash-compatible** — `jaksh -c "cmd"` chạy 1 lệnh rồi exit (tắt banner / greeting / timing / history); kèm `-l/--login`, `-i/--interactive`, `-V/--version`, `-h/--help`, positional script file, `--` separator. Đủ để JakShell làm shell trong **VSCode**, task runner, CI, login shell.
- **Curl one-liner install** — `bootstrap.sh` clone repo + chạy `install.sh --yes`. Tuỳ chỉnh qua `JAKSH_DIR / JAKSH_REF / PREFIX`. Hỗ trợ cả `wget`.
- **Markdown → ANSI renderer** (`src/markdown.rs`) — `jak version` / `jak version all` render CHANGELOG đẹp: headings màu, bullet `•`, inline code, bold, code fence với prefix `│`, link `[text](url)`. Iterate UTF-8 char để diacritics Việt không bị phá.

### Đã sửa

- `examples/jakshrc`: dòng `alias ..=cd ..` thiếu nháy kép — từ `..` thứ 2 bị parse thành arg query nên alias builtin in `alias ..='cd'` ra **stdout**, làm bẩn output của `jaksh -c`. Đổi thành `alias ..="cd .."` + comment cảnh báo.

### Tài liệu

- README "Cài đặt" — section curl one-liner + env vars + `wget` variant.
- README "Đặt làm shell mặc định" — guide 3 bước, kiểm tra `dscl`/`$SHELL`, khôi phục zsh/bash, bảng troubleshooting 5 case (PAM, terminal override, lỗi config, TTY recovery).
- Banner tip mới: `jak help` / `explain` thay cho tip về `~/.jakshrc.toml`.

---

## [v1.0.2] — i18n + trải nghiệm gõ lệnh thông minh

### Đã thêm

- **🌐 i18n 6 ngôn ngữ** — `vi / en / kr / jp / cn / th`. Lệnh `jak lang <code>` đổi & lưu vĩnh viễn vào `~/.config/jaksh/language`.
  - **Toàn bộ 106 entry `explain`** đã được dịch (summary + flags + examples + note) — tổng hơn 5000 chuỗi.
  - Triết lý: thuật ngữ dev (PID, branch, permissions, …) giữ nguyên tiếng Anh; chỉ prose là dịch.
- **Inline autosuggest** (fish-style ghost text) — gõ vài ký tự thấy gợi ý mờ ngay sau cursor, nhấn `→` để chấp nhận. Nguồn: history → builtin → alias → `jak`.
- **Tab completion list-mode** — Tab lần 1 in danh sách candidates + complete common prefix; Tab tiếp theo cycle qua từng option (bash/zsh style).
- **Icon trong tab completion** — `⚙ builtin · ↪ alias · 🔖 bookmark · ★ jak utility · $ PATH · 📁 dir · 📄 file · ▶ exec` để phân biệt nhanh.
- **Context-aware path completion** — `cd Ca<Tab>` chỉ folders; lệnh khác vẫn hiện cả files + folders.
- **Hint khi lệnh fail** — sau exit != 0 in giải thích mã (`126 = thiếu quyền x`, `137 = SIGKILL OOM`, `139 = segfault`, …) + gợi ý `--help` hoặc `chmod +x`. Skip cho lệnh non-zero-by-design (grep/test/diff/false/…).
- **`ls` tự tô màu mặc định** — set `CLICOLOR/LSCOLORS/LS_COLORS` env + alias `ls -Gp` (macOS) / `ls --color=auto -p` (Linux). Thư mục bold blue + `/` cuối, file mặc định, executable green, symlink cyan.

### Đã sửa

- Bug normalize tên explain với ký tự `/` (vd `docker stop / start / restart`) khiến i18n key không khớp → toàn bộ rơi về tiếng Việt. Đã fix: `normalize_name()` mới thay mọi ký tự không phải alphanumeric/'-' bằng `_`.
- Bug i18n thiếu `summary` cho 35 lệnh (uptime, who, git subs, docker subs, …) — đã bổ sung.

### Cấu hình mới

```toml
[timing]
show_hint = true        # in hint sau lệnh fail (default true)
```

```bash
~/.config/jaksh/language    # lưu mã ngôn ngữ đã chọn (vi/en/kr/jp/cn/th)
```

---

## [v1.0.1] — Tự cập nhật & thông tin version

### Đã thêm

- **`jak version`** — in thông tin chi tiết phiên bản: tag git, commit SHA, commit date, build date (UTC), rustc, target triple, tác giả, link repo. Kèm luôn section CHANGELOG mới nhất.
- **`jak version all`** — như trên + toàn bộ CHANGELOG (nhúng vào binary qua `include_str!`).
- **`jak self-update`** (alias: `jak upgrade`, `jak selfupdate`) — tự `git pull --rebase` ở thư mục source rồi chạy `./install.sh --yes`. Sau khi xong tự hiện section CHANGELOG mới.
- **`install.sh`** lưu đường dẫn source vào `~/.config/jaksh/source-path` để `jak self-update` biết nơi pull.
- **`build.rs`** mở rộng: nhúng thêm `JAKSH_COMMIT_HASH`, `JAKSH_COMMIT_DATE`, `JAKSH_BUILD_DATE`, `JAKSH_RUSTC` vào binary.

### Đã sửa

- `install.sh` thiếu định nghĩa `SCRIPT_DIR` ở phiên bản rewrite trước — đã thêm lại.

### Tài liệu

- README có section "Cập nhật bản mới" với 2 cách (tự động qua `jak self-update`, hoặc thủ công).

---

## [v1.0.0] — Phát hành đầu tiên 🎉

> Bản chính thức đầu tiên của **JakShell** — một shell viết bằng Rust cho macOS & Linux, vừa giữ cú pháp POSIX vừa thêm bộ công cụ tiếng Việt thân thiện cho cả developer và người dùng phổ thông.

### Điểm nổi bật

- ⚡ **Nhanh & gọn**: binary release ~1.4 MB, startup ~10 ms.
- 🇻🇳 **Lowtech-friendly**: trợ giúp / lỗi / chú thích bằng tiếng Việt — thuật ngữ kỹ thuật giữ nguyên gốc.
- 📚 **`explain`** — học 70+ lệnh Unix với usage / tham số / ví dụ + chú thích giá trị output thật.
- 🎨 **`--jak`** — tô màu + format lại output cho ls / ps / df / du / git.
- 🔖 **`bookmark`** — đặt tên cho lệnh dài, gọi qua `jak <tên>`.
- 🧰 **`jak <…>`** — 10+ tiện ích: clean, backup, update, find, open, sysinfo, ip, weather, theme, git.
- 🌳 **Smart git prompt** — branch, dirty `*`, ahead `↑N`, behind `↓N`, stash `⚑N`, state `MERGE/REBASE/PICK`.
- 🎭 **17 theme** dựng sẵn, có lưu lựa chọn vĩnh viễn.
- 🙋 **Greeting** theo giờ tiếng Việt + mẹo ngẫu nhiên khi mở shell.
- ⏱ **Timing** — `⏱ X ms` sau mỗi lệnh, đỏ khi exit code != 0.
- 💡 **Did-you-mean** — gợi ý lệnh đúng khi gõ sai.

---

### Đã thêm

#### Shell core (POSIX-compatible)
- Cú pháp: `|`, `&&`, `||`, `;`, `&`, `>`, `>>`, `<`, `2>`, `2>>`, `&>`
- Biến: `$VAR`, `${VAR}`, `$?`; tilde `~`; glob `* ? [abc]`
- Quote: `'...'` (literal) và `"..."` (cho phép `$VAR`); **POSIX-correct**: quote chặn glob expansion
- Job nền & quản lý job: `&`, `jobs`, `fg`, `bg`, `kill`
- 23 builtin: `cd`, `pwd`, `exit`, `export`, `unset`, `alias`, `unalias`, `set`, `echo`, `source`, `.`, `history`, `jobs`, `fg`, `bg`, `kill`, `help`, `?`, `which`, `true`, `false`, `explain`, `bookmark`
- Redirect hoạt động cho cả builtin (qua `dup2`)
- Mở rộng alias 1 cấp (giống bash)

#### `explain` — chú thích lệnh
- **70+ lệnh** được chú thích với format thống nhất: tóm tắt + cú pháp + tham số + ví dụ + ghi chú
- **Live annotation** cho `ls -l`, `ps`, `df`, `du`, `free` — parse output thật và chú thích từng giá trị (decode permissions, map header với row đầu)
- **`skip_run`** cho các lệnh destructive (`rm`, `mv`, `chmod`, `kill`, …) — chỉ in legend, không tự chạy
- Nhóm theo chủ đề trong `explain list`:
  - Điều hướng, file management, viewing, text filter, process, disk, mạng, archive, system, **Git** (26 sub), **Docker** (20 sub), **SSH family**

#### `--jak` — pretty output
- `ls -la --jak`: permissions tô từng ký tự, icon dir/exec/symlink, size theo đơn vị
- `ps aux --jak`: USER màu theo root/user, PID cyan, %CPU/%MEM theo ngưỡng (xanh→vàng→đỏ), STAT theo trạng thái
- `df -h --jak`: Use% xanh→vàng→đỏ (≥50/75/90), size theo unit, mount path xanh
- `du -sh --jak`: căn cột size, thư mục bold blue
- `git status --jak`: bố cục theo section (Staged / Modified / Untracked / Conflict) + icon `✎ + ✗ ⚠`
- `git branch --jak`: `●` đánh dấu branch hiện tại

#### `jak <…>` — bộ tiện ích
- `jak clean [--dry]` — xoá file tạm / cache
- `jak backup <thư_mục>` — nén `.tar.gz` với tên kèm ngày-giờ
- `jak update` — auto-detect & chạy `brew / port / apt / dnf / pacman / zypper / apk`
- `jak find` — tìm tự nhiên: `file / dir / text / big / recent / empty`, hỗ trợ `in` / `trong`, glob hoặc substring
- `jak open` — **101 app alias** (chrome / vscode / slack / zalo / figma / postman / …) + **10 URL alias** (github / chatgpt / claude / gmail / …); macOS dùng `open -a`, Linux spawn detached
- `jak sysinfo` — OS, CPU, RAM, đĩa
- `jak ip` — IP nội bộ + public, thử nhiều fallback (`ipconfig / hostname / ip / curl / wget`)
- `jak weather [tp]` — thời tiết qua wttr.in
- `jak theme <name>` — **17 theme** dựng sẵn (default, ocean, forest, sunset, mono, dracula, nord, monokai, solarized, gruvbox, tokyo-night, catppuccin, rose-pine, cyberpunk, retro, paper, light) — **lưu lựa chọn** vào `~/.config/jaksh/theme`
- `jak git <…>` — 9 shortcut: `save`, `wip`, `sync`, `publish`, `amend`, `uncommit`, `undo`, `unstage`, `clean-branches`
- `jak <bookmark>` — chạy bookmark đã đặt

#### `bookmark` builtin
- `bookmark <name> <cmd ...>` — tạo / cập nhật
- `bookmark` / `bookmark list` — liệt kê
- `bookmark show <name>` / `bookmark del <name>` — xem / xoá
- Lưu tại `~/.config/jaksh/bookmarks.toml`
- Hỗ trợ shell syntax đầy đủ (pipe, redirect, biến) — chạy qua lexer/parser/executor
- Hiện trong `jak help` để dễ nhớ

#### Smart git prompt
- 1 lần gọi `git status --branch --porcelain=v2` — lấy mọi thông tin
- Hiển thị: branch (cyan, magenta nếu detached), dirty `*` (vàng), conflict `⚠N` (đỏ), ahead `↑N` (xanh), behind `↓N` (đỏ), stash `⚑N` (tím)
- State: `MERGE / REBASE / PICK / REVERT / BISECT` đọc từ `.git/`

#### Trải nghiệm khởi động
- **Greeting** theo giờ tiếng Việt: 🌅 sáng / ☀️ trưa / 🌤 chiều / 🌆 tối / 🌙 đêm khuya
- Tên user + Thứ Hai…Chủ Nhật + ngày + giờ
- **Tip ngẫu nhiên** từ pool 15 mẹo, đổi mỗi lần mở
- Cấu hình bật/tắt từng phần qua `[greeting]`

#### Banner thông tin (mới ở `?` / `jak` / `explain`)
- Tên + version (git describe) + tagline
- Liệt kê tính năng chính
- Credit: **Developed by Jarvis Phong Tran**
- URL repo

#### Timing display
- `⏱ X µs / ms / s / m+s / h+m+s` sau mỗi lệnh (in stderr, không phá pipe)
- Đỏ kèm `✗ exit N` khi exit code khác 0
- Cấu hình `[timing]`: enabled, threshold_ms, show_status

#### Did-you-mean
- Jaro-Winkler distance, ngưỡng 0.86
- Bỏ qua namespace nội bộ (`jak`, `explain`)
- Triggered khi exit code = 127 hoặc lỗi parse

#### Defensive command checks
- `has_cmd()` helper kiểm tra binary trên PATH
- Tin nhắn lỗi vàng tiếng Việt khi thiếu deps
- `try_run` cho thông tin phụ — báo "↳ bỏ qua: X không có" thay vì crash
- `jak update` chỉ list package manager đúng OS (macOS: brew/port; Linux: apt/dnf/pacman/zypper/apk/brew)

#### Tab completion
- Builtin, alias, PATH binary
- `jak <Tab>` → subcommands
- `bookmark <Tab>`, `explain <Tab>`, `jak find <Tab>`, `jak git <Tab>`
- `--jak` gợi ý cho ls/ps/df/du/git
- Path completion thông thường

#### Cấu hình
- `~/.jakshrc.toml` — TOML: `prompt`, `[theme]`, `[timing]`, `[greeting]`, `[aliases]`, `[env]`
- `~/.jakshrc` — script khởi động (shell syntax)
- `~/.config/jaksh/theme` — theme đã chọn qua `jak theme`
- `~/.config/jaksh/bookmarks.toml` — bookmark
- `~/.config/jaksh/history` — lịch sử lệnh

#### Installer (`install.sh`)
- Auto-detect OS + package manager
- Tự source `~/.cargo/env` nếu rustup đã cài
- Cài Rust qua rustup nếu thiếu (có xác nhận)
- Cài runtime deps qua pkg manager: `git`, `curl`, `tar` (bắt buộc); `ripgrep` (cải thiện `jak find text`); `xdg-utils` (Linux, cho `jak open`)
- Cờ: `--yes`, `--no-deps`, `--prefix <path>`
- Idempotent — chạy lại không cài lại gói đã có
- PATH check + hướng dẫn next steps

#### Build system
- `build.rs` đọc `git describe --tags --always --dirty=-dirty` → biến `JAKSH_VERSION`
- Hiển thị trong welcome banner & `jak help`
- Fallback `CARGO_PKG_VERSION` nếu không có git

---

### Yêu cầu hệ thống

- **macOS** (Intel hoặc Apple Silicon) hoặc **Linux** (Debian/Ubuntu/Fedora/Arch/openSUSE/Alpine)
- **Rust** ≥ 1.70 (sẽ tự cài qua rustup nếu chưa có)
- Tuỳ chọn: `git`, `tar`, `curl`, `ripgrep`, `xdg-utils` (Linux) — `install.sh` sẽ tự cài

### Cài đặt

```bash
git clone https://github.com/mockingbitch/jakshell.git
cd jakshell
./install.sh
```

Cài hết deps không hỏi:
```bash
./install.sh --yes
```

Đổi nơi cài binary:
```bash
PREFIX=/usr/local/bin ./install.sh
```

### Đặt làm shell mặc định

```bash
echo "$HOME/.local/bin/jaksh" | sudo tee -a /etc/shells
chsh -s "$HOME/.local/bin/jaksh"
```

### Bắt đầu

```bash
?                       # banner + trợ giúp
jak                     # tất cả tiện ích jak
explain                 # các lệnh đã có chú thích
explain ls -la          # ví dụ thực tế
ls -la --jak            # output đẹp
jak theme list          # chọn giao diện
```

### Bộ thông số

- Binary release: **~1.4 MB**
- Startup: **~10 ms**
- Memory baseline: ~5 MB
- Dependencies (Cargo): rustyline, crossterm, anyhow, dirs, shellexpand, glob, nix, libc, serde, toml, chrono, strsim, which

---

### Cảm ơn

- **Tác giả & maintainer**: Jarvis Phong Tran ([@mockingbitch](https://github.com/mockingbitch))
- Cảm ơn dự án **rustyline** đã cho line editor nền tảng vững chắc.
- Cảm ơn hệ sinh thái **Rust** đã làm CLI phát triển trở nên dễ chịu.

---

[v1.0.3]: https://github.com/mockingbitch/jakshell/releases/tag/v1.0.3
[v1.0.2]: https://github.com/mockingbitch/jakshell/releases/tag/v1.0.2
[v1.0.1]: https://github.com/mockingbitch/jakshell/releases/tag/v1.0.1
[v1.0.0]: https://github.com/mockingbitch/jakshell/releases/tag/v1.0.0
