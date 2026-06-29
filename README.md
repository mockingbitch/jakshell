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
  - [`jak news` — tin tức + AI tóm tắt](#jak-news--tin-tức--ai-tóm-tắt)
  - [`jak git` — workflow git rút gọn](#jak-git--workflow-git-rút-gọn)
  - [`jak version` & `jak self-update`](#jak-version--jak-self-update)
  - [`bookmark` — đặt tên cho lệnh dài](#bookmark--đặt-tên-cho-lệnh-dài)
- [Prompt thông minh](#prompt-thông-minh)
- [Đo thời gian thực thi](#đo-thời-gian-thực-thi)
- [Did-you-mean & failure hints](#did-you-mean--failure-hints)
- [Tab completion & inline suggestions](#tab-completion--inline-suggestions)
- [Ngôn ngữ](#ngôn-ngữ)
- [`ls` tự tô màu](#ls-tự-tô-màu)
- [Cấu hình](#cấu-hình)
- [File & thư mục](#file--thư-mục)
- [Đặt làm shell mặc định](#đặt-làm-shell-mặc-định)
- [Versioning](#versioning)
- [Cấu trúc dự án](#cấu-trúc-dự-án)
- [License](#license)

---

## Tính năng nổi bật

- **POSIX-compatible** — pipe `|`, `&&`, `||`, `;`, redirect `> >> < 2> 2>> &>`, job nền, glob, biến, tilde, quote chuẩn.
- **`explain <lệnh>`** — usage / tham số / ví dụ + chú thích từng cột output thật (ls, ps, df, du, free). 70+ lệnh.
- **`--jak`** trên `ls / ps / df / du / git status / git branch` — tô màu + format lại.
- **`jak …`** — `clean / backup / update / find / open / sysinfo / ip / weather / news / theme / git / version / self-update / lang`.
- **`jak news`** — crawl tin từ RSS uy tín rồi dùng AI (Claude) phân loại theo chủ đề + tóm tắt ý chính. Key do bạn cung cấp; thiếu key vẫn xem được tin thô.
- **`bookmark`** — đặt tên cho lệnh dài, chạy qua `jak <name>`.
- **Smart git prompt** — branch + dirty `*` + `↑↓` ahead/behind + stash `⚑N` + state `MERGE/REBASE`.
- **Trải nghiệm gõ lệnh** — inline autosuggest, tab completion list-mode với icon, context-aware path, timing `⏱`, did-you-mean, hint khi fail, `ls` tự tô màu.
- **🌐 6 ngôn ngữ** — `vi / en / kr / jp / cn / th` (`jak lang <code>`).
- **17 theme** dựng sẵn, lưu lựa chọn vĩnh viễn.
- **CLI args** — `jaksh -c "cmd"`, `-l/--login`, `-V/--version`, script file → dùng được trong VSCode / task runner / CI.
- **`jak version`** render CHANGELOG markdown đẹp; **`jak self-update`** = pull + rebuild 1 lệnh.

Binary release ~1.4 MB · startup ~10 ms.

---

## Cài đặt

**Yêu cầu**: macOS hoặc Linux. Rust sẽ tự cài nếu thiếu. Tuỳ chọn: `git / tar / curl / ripgrep` cho một số tiện ích `jak`.

### Cài nhanh bằng curl

```bash
curl -fsSL https://raw.githubusercontent.com/mockingbitch/jakshell/master/bootstrap.sh | bash
```

`bootstrap.sh` clone repo về `~/.jakshell` rồi gọi `install.sh --yes` (cài Rust nếu thiếu → `cargo build --release` → copy binary vào `~/.local/bin/jaksh` → tạo config mẫu → ghi source path để `jak self-update` biết nơi pull). Cần `git` sẵn trên máy.

Biến môi trường:

| Biến | Mặc định | Mục đích |
|------|----------|----------|
| `JAKSH_DIR` | `~/.jakshell` | Thư mục clone |
| `JAKSH_REF` | `master` | Branch/tag để checkout |
| `PREFIX` | `~/.local/bin` | Nơi cài binary |

`wget` variant:

```bash
wget -qO- https://raw.githubusercontent.com/mockingbitch/jakshell/master/bootstrap.sh | bash
```

### Cài thủ công

```bash
git clone https://github.com/mockingbitch/jakshell.git && cd jakshell
./install.sh                # hoặc: cargo build --release
~/.local/bin/jaksh
```

### Cập nhật bản mới

```bash
jak self-update             # khuyên dùng — pull + rebuild + cài lại
# hoặc thủ công:
cd ~/.jakshell && git pull --rebase && ./install.sh
```

Mở terminal mới để dùng bản vừa cập nhật.

**Tự động báo bản mới:** JakShell tự kiểm tra phiên bản mới khi mở shell — việc
gọi mạng **chạy nền** nên không làm chậm khởi động, và thông báo (nếu có bản
mới) hiện ở lần mở kế tiếp. Khi có bản mới, shell hỏi một lần cho mỗi version:
**có** cập nhật ngay · **để sau** (nhắc lại sau vài giờ) · **bỏ qua bản này** ·
**không**. Tuỳ chỉnh hoặc tắt qua section `[update]` trong `~/.jakshrc.toml`
(xem [Cấu hình](#cấu-hình)).

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

**70+ lệnh** đã có chú thích: điều hướng (`cd / ls / find`), file (`cp / mv / rm / chmod / chown / ln / …`), xem & filter (`cat / head / tail / grep / sort / uniq / wc / cut / tr / xargs`), process (`ps / top / kill / pkill`), đĩa (`df / du / free / stat / lsof`), mạng (`ssh* / scp / curl / wget / ping / netstat / ss / ip / ifconfig`), nén (`tar / zip`), hệ thống (`uptime / who / date / env / alias / history / which / man`), **Git** (26 sub), **Docker** (20 sub). Chạy `explain list` xem toàn bộ.

> Lệnh destructive (`rm / mv / chmod / kill / docker exec / …`) chỉ in legend, KHÔNG tự chạy.

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
| `jak news …` | Tin tức từ RSS + AI phân loại & tóm tắt (xem [jak news](#jak-news--tin-tức--ai-tóm-tắt)) |
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

### `jak news` — tin tức + AI tóm tắt

Crawl tin từ các nguồn **RSS** uy tín, rồi dùng **AI (Claude)** phân loại theo chủ đề và tóm tắt 2–3 ý chính mỗi bài — hiển thị gọn ngay trong terminal.

```bash
jak news                 # tin mới (tự crawl + tóm tắt nếu cache đã cũ)
jak news refresh         # ép crawl + tóm tắt lại ngay
jak news cong-nghe       # lọc theo chủ đề (bỏ dấu cũng được)
jak news 3               # chi tiết bài số 3 (ý chính + link)
jak news sources         # xem nguồn RSS + cấu hình hiện tại
jak news help
```

**Bật AI** — phần phân loại + tóm tắt cần API key Anthropic của **bạn**:

```bash
export ANTHROPIC_API_KEY=sk-ant-...      # hoặc đặt [news] api_key trong ~/.jakshrc.toml
jak news refresh
```

Thiếu key → JakShell **vẫn crawl và hiện tin thô** (tiêu đề + mô tả gốc), chỉ bỏ phần AI, không bao giờ chặn.

Cơ chế:
- **Nguồn**: danh sách RSS trong `[news] sources` — mặc định VnExpress / Tuổi Trẻ / Thanh Niên; thêm/bớt tuỳ ý (phần lớn báo VN đều có endpoint `.rss`).
- **Rẻ & ổn định**: gộp nhiều bài vào **1 request** với *structured output* (JSON schema) — model mặc định `claude-haiku-4-5`. Crawl qua `curl`, parser RSS/Atom không cần thư viện ngoài.
- **Cache** tại `~/.config/jaksh/news-cache.json`, tươi trong `ttl_minutes` (mặc định 30). Trong khoảng đó `jak news` đọc cache, **không gọi AI lại** (tránh tốn tiền). Không có tiến trình nền — AI chỉ chạy khi bạn chủ động gọi và cache đã hết hạn.
- **Chủ đề**: Thời sự / Thế giới / Kinh tế / Công nghệ / Khoa học / Thể thao / Giải trí / Sức khỏe / Giáo dục / Khác.

Cấu hình đầy đủ ở section `[news]` (xem [Cấu hình](#jakshrctoml--cấu-hình-toml)).

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

### `jak version` & `jak self-update`

**`jak version`** — in thông tin chi tiết về phiên bản đang chạy + section CHANGELOG mới nhất:

```
JakShell  v1.0.1

  Commit:      a1b2c3d
  Commit date: 2026-06-06 13:23:29 +0700
  Built:       2026-06-06 06:34:30 UTC
  Rust:        rustc 1.96.0
  Target:      aarch64-macos

  Author:      Jarvis Phong Tran
  Repo:        https://github.com/mockingbitch/jakshell
  Update:      jak self-update

──────────────────────────────────────────
📝 Có gì mới trong bản này:
## [v1.0.1] — ...
```

- `jak version` — version info + section CHANGELOG mới nhất
- `jak version all` — version info + toàn bộ CHANGELOG
- Bí danh: `jak --version`, `jak -v`, `jak changelog`, `jak whatsnew`
- CHANGELOG được **nhúng vào binary** (`include_str!`) — không cần source repo để xem.

**`jak self-update`** — cập nhật JakShell tự động:

```bash
jak self-update
```

Quy trình:
1. Đọc đường dẫn source từ `~/.config/jaksh/source-path` (do `install.sh` lưu lúc cài).
2. Chạy `git fetch --tags` → `git pull --rebase`.
3. Chạy `./install.sh --yes` để rebuild + cài lại binary.
4. In `Đã cập nhật: vX.Y.Z → vA.B.C` + section CHANGELOG mới nhất.
5. Nhắc mở terminal mới để dùng bản vừa update.

Nếu thư mục source mất hoặc chưa có file `source-path`, lệnh sẽ in hướng dẫn cập nhật thủ công.

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

## Did-you-mean & failure hints

**Did-you-mean** — gõ sai → gợi ý lệnh đúng (jaro-winkler ngưỡng 0.86; bỏ qua namespace nội bộ `jak` / `explain`):

```
$ gitt status
jaksh: không tìm thấy lệnh: gitt
💡 có phải bạn muốn: git?
```

**Failure hints** — sau mỗi lệnh exit ≠ 0, in giải thích mã + gợi ý sửa:

```
$ ./noexec.sh
jaksh: Permission denied
⏱  1 ms  ✗ exit 126
💡 Mã 126: file tồn tại nhưng KHÔNG execute được (thiếu quyền x).
   Sửa: chmod +x <file>

$ ls --bogus
⏱  4 ms  ✗ exit 1
💡 Mã 1: lỗi chung — xem stderr ở trên để biết chi tiết.
   Thử: ls --help  hoặc  man ls
```

Bảng mã có giải thích: `1 / 2 / 126 / 128 / 130 / 137 / 139 / 143` + signal kill 129–191. Skip cho lệnh non-zero-by-design: `grep / test / diff / false / cmp / ...`.

Tắt bằng `[timing] show_hint = false` trong `~/.jakshrc.toml`.

---

## Tab completion & inline suggestions

**Inline autosuggest** (fish-style): khi gõ, JakShell hiện ghost text mờ ngay sau cursor. Nhấn `→ (Right Arrow)` để chấp nhận. Nguồn (theo thứ tự ưu tiên):

1. History — lệnh gần đây nhất khớp prefix
2. Builtin (`cd`, `alias`, `explain`, `bookmark`, …)
3. Alias đã đặt
4. Prefix `jak`

**Tab completion list-mode**:

| Lần | Hành vi |
|-----|---------|
| Tab 1 | In danh sách candidates + extend đến common prefix |
| Tab 2+ | Cycle qua từng option |

**Icon phân loại trong list**:

| Icon | Loại |
|------|------|
| `⚙` | Builtin |
| `↪` | Alias (kèm preview lệnh thật) |
| `🔖` | Bookmark (kèm preview) |
| `★` | Jak utility / git workflow / explain / pretty / search |
| `$` | PATH binary |
| `📁` | Directory |
| `📄` | File |
| `▶` | Executable |

**Context-aware path**: `cd Ca<Tab>` → chỉ folders (lệnh `cd / pushd / popd / rmdir / chdir`); các lệnh khác hiện cả files + folders để navigation hoạt động.

---

## Ngôn ngữ

JakShell hỗ trợ **6 ngôn ngữ**:

```bash
jak lang               # info ngôn ngữ + danh sách
jak lang en            # đổi sang English + lưu vĩnh viễn
jak lang reset         # về mặc định (vi)
```

| Code | Ngôn ngữ | Cờ |
|------|----------|-----|
| `vi` | Tiếng Việt (mặc định) | 🇻🇳 |
| `en` | English | 🇺🇸 |
| `kr` | 한국어 | 🇰🇷 |
| `jp` | 日本語 | 🇯🇵 |
| `cn` | 中文 | 🇨🇳 |
| `th` | ภาษาไทย | 🇹🇭 |

**Triết lý**: chỉ dịch prose (chào, lời nhắc, mô tả). Thuật ngữ dev (`PID`, `branch`, `permissions`, `RSS`, `staged`, `glob`, …) giữ nguyên tiếng Anh để khớp với tài liệu và phản xạ của developer.

Toàn bộ **106 entry `explain`** đã được dịch đầy đủ (summary + flags + examples + note). Banner / greeting / help / `jak version` cũng đa ngôn ngữ.

Lưu tại `~/.config/jaksh/language` (1 dòng chứa mã).

---

## `ls` tự tô màu

Gõ `ls` (không cần cờ) là đã thấy phân biệt:
- Thư mục: **bold blue** + dấu `/` cuối
- File thường: màu mặc định
- Executable: **green**
- Symlink: **cyan**

Đạt được bằng:
- Env var tự đặt: `CLICOLOR=1`, `LSCOLORS` (BSD/macOS), `LS_COLORS` (GNU/Linux). Không ghi đè nếu user đã set.
- Alias mặc định: `ls=ls -Gp` (macOS) / `ls --color=auto -p` (Linux). User override được qua `[aliases]` trong `~/.jakshrc.toml`.

Khác với `ls -la --jak` (full reformat + chú thích từng cột): chỉ tô màu nhẹ. Đủ dùng hàng ngày.

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

[ui]
blank_line = true           # dòng trống giữa các khối lệnh (dễ nhìn lại output cũ)
separator = "none"          # "line" = thêm đường kẻ mờ ngang giữa các khối

[timing]
enabled = true
threshold_ms = 0
show_status = true

[greeting]
enabled = true
show_greeting = true        # dòng "Chào buổi …"
show_tip = true             # mẹo ngẫu nhiên
name = ""                   # rỗng = lấy $USER

[update]
check = true                # kiểm tra bản mới khi mở shell (gọi mạng CHẠY NỀN)
interval_hours = 24         # tối thiểu bao nhiêu giờ giữa 2 lần gọi mạng
remind_hours = 8            # chọn "để sau" → im bấy nhiêu giờ rồi nhắc lại
prompt = true               # false = chỉ in 1 dòng nhắc, không hỏi tương tác

[news]                      # jak news — tin tức + AI tóm tắt
sources = [                 # danh sách link RSS (báo VN đa số có endpoint .rss)
  "https://vnexpress.net/rss/tin-moi-nhat.rss",
  "https://tuoitre.vn/rss/tin-moi-nhat.rss",
  "https://thanhnien.vn/rss/home.rss",
]
max_items = 20              # số bài tối đa mỗi lần làm mới
ttl_minutes = 30            # cache còn tươi bấy nhiêu phút (không gọi AI lại)
model = "claude-haiku-4-5"  # model Claude cho phân loại + tóm tắt
ai = true                   # false = chỉ crawl + hiện tin thô (không tốn tiền)
api_key = ""                # nên để rỗng & dùng env ANTHROPIC_API_KEY (ưu tiên)

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
alias ..="cd .."        # nháy kép bắt buộc khi value > 1 từ
```

---

## File & thư mục

| Đường dẫn | Mục đích |
|-----------|----------|
| `~/.jakshrc.toml` | Cấu hình chính (theme, prompt, ui, alias, env, timing, greeting) |
| `~/.jakshrc` | Script khởi động (chạy mỗi khi mở shell) |
| `~/.config/jaksh/history` | Lịch sử lệnh |
| `~/.config/jaksh/bookmarks.toml` | Bookmark do `bookmark` quản lý |
| `~/.config/jaksh/news-cache.json` | Cache tin tức của `jak news` (có TTL) |

---

## Đặt làm shell mặc định

Thay `bash` / `zsh` bằng JakShell làm login shell.

### Bước 1 — đảm bảo binary đã cài

```bash
ls -l "$HOME/.local/bin/jaksh"     # phải tồn tại & có quyền x
```

Chưa có? Chạy `./install.sh` hoặc one-liner `curl … bootstrap.sh | bash` (xem [Cài đặt](#cài-đặt)).

### Bước 2 — đăng ký với hệ thống

`chsh` chỉ chấp nhận shell có mặt trong `/etc/shells`:

```bash
# Idempotent: chỉ append nếu chưa có
grep -qxF "$HOME/.local/bin/jaksh" /etc/shells \
  || echo "$HOME/.local/bin/jaksh" | sudo tee -a /etc/shells
```

### Bước 3 — đổi shell cho user hiện tại

```bash
chsh -s "$HOME/.local/bin/jaksh"
```

`chsh` sẽ hỏi mật khẩu user. **Mở terminal tab/window mới** — JakShell sẽ là login shell.

Kiểm tra:

```bash
dscl . -read /Users/$USER UserShell   # macOS
echo $SHELL                            # Linux/macOS, cần terminal mới
```

### Quay lại zsh / bash

```bash
chsh -s /bin/zsh        # macOS default
chsh -s /bin/bash       # nhiều distro Linux
```

### Sự cố thường gặp

| Triệu chứng | Cách xử lý |
|-------------|-----------|
| `chsh: non-standard shell` | Đường dẫn `jaksh` chưa có trong `/etc/shells` → chạy lại Bước 2 |
| `chsh: PAM authentication failed` | Sai password user. Lưu ý KHÔNG dùng `sudo chsh` — `chsh` đổi shell cho chính user gọi nó |
| Terminal mới vẫn vào zsh | Một số terminal (iTerm2, VSCode) override shell trong profile/settings — sửa trong `Preferences → Profiles → Command` |
| Banner JakShell chạy nhưng không gõ được lệnh nào | Có lỗi trong `~/.jakshrc.toml` hoặc `~/.jakshrc` — đổi tên đi (`mv ~/.jakshrc.toml ~/.jakshrc.toml.bak`) rồi thử lại |
| Lỡ tay set shell sai → đăng nhập lỗi | Đăng nhập qua TTY khác (Linux: Ctrl+Alt+F2) hoặc Recovery Mode rồi `chsh -s /bin/bash` |

> **Khuyến nghị**: trước khi `chsh`, mở 1 terminal jaksh thủ công (`~/.local/bin/jaksh`) và dùng vài ngày. Khi quen mới đổi default — tránh kẹt nếu có bug.

---

## Versioning

`build.rs` nhúng `git describe --tags --always --dirty=-dirty` vào binary. Xem chi tiết bằng `jak version` (info ngắn + CHANGELOG mới nhất) hoặc `jak version all` (toàn bộ CHANGELOG). Render qua module markdown nội bộ — headings màu, bullet, code fence, link.

Format hiển thị: `v1.0.2` (sạch) · `v1.0.2-3-g91b4d81` (3 commit sau tag) · `v1.0.2-dirty` (có thay đổi chưa commit) · short SHA nếu chưa có tag.

Release version mới:
```bash
git tag -a vX.Y.Z -m "Release X.Y.Z" && git push origin vX.Y.Z
gh release create vX.Y.Z --notes-file CHANGELOG.md
```

---

## Cấu trúc dự án

```
JakShell/
├── Cargo.toml           # package + dependencies
├── build.rs             # nhúng version từ git vào binary
├── bootstrap.sh         # one-liner curl install (clone + run install.sh)
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
