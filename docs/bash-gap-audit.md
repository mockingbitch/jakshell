# Audit: jakshell so với bash

*Ngày audit: 2026-10-22 · bản `v1.0.11` + các sửa đổi history/curl chưa commit · so với GNU bash 5.x*

## Tóm tắt

- **Bộ so sánh tự động: 50/160 case khớp bash.** Mỗi case chạy `bash -c` và `jaksh -c` trong thư mục + HOME tạm, so stdout + exit code.
- jaksh hiện là **trình chạy lệnh đơn + pipeline + `&&`/`||`/`;`**. AST chỉ có `SimpleCommand → Pipeline → Program` ([parser.rs](../src/parser.rs)). Không có `if`/`for`/`while`/`case`/function, `${…}`, `$((…))`, here-doc hay `2>&1`, nên **gần như không script bash nào chạy được** (kể cả `install.sh` của chính repo).
- **Lỗi tương tác nghiêm trọng nhất: nhấn Ctrl-C khi lệnh đang chạy giết luôn jaksh.** Đã tái hiện qua pty: `sleep 5` rồi ^C, shell thoát với SIGINT. Ctrl-Z cũng không tạm dừng được job.
- **Một số lỗi rẻ mà ảnh hưởng rộng:**
  - `$?` không cập nhật giữa các lệnh trên cùng một dòng.
  - `cmd & cmd2` báo lỗi parse.
  - Mọi biến shell đều bị export sang tiến trình con.
  - `jaksh -c 'exit'` in "tạm biệt 👋" ra stdout.
- **Điểm mạnh giữ nguyên:** glob, quoting, command substitution, redirect cơ bản, `cd -`, các tiện ích tương tác (gợi ý mờ, timing, giải thích exit code, `explain`, `--jak`).

## Tiến độ

| Mốc | Khớp bash (`run.py`) | `signals.py` |
|---|---|---|
| Audit ban đầu | 50/160 | Ctrl-C giết shell |
| Job control | 54/160 | 30/30 |
| **Đợt 1 xong** | **94/160** | **30/30** |

**Đợt 1 đã làm:**
- **Job control đầy đủ:** process group riêng; Ctrl-C/Ctrl-Z/Ctrl-\ không bao giờ giết hay treo shell; `fg`/`bg`/`jobs -l`/`wait`/`disown`/`kill %N`; khôi phục chế độ terminal; cảnh báo khi thoát còn job dừng.
- **Exit code:** `$?` đúng sau từng lệnh; Ctrl-C bỏ phần còn lại của dòng.
- **Cú pháp:** `&` là dấu phân tách, `! pipeline`.
- **Redirect:** `N>&M`, `>&-`, `&>>`, `>|`, `<>`, `N<`, `exec 3>file`, `> file` không kèm lệnh; here-doc `<<EOF` / `<<-` / `<<'EOF'`; here-string `<<<`; `noclobber`.
- **Biến:** biến shell tách khỏi env (chỉ `export` mới sang tiến trình con); `export -n`, `readonly`, `declare`/`typeset` (không mảng), `x+=`.
- **Tham số & biến đặc biệt:** `$0 $1… $# $@ $* $$ $! $- $RANDOM $SECONDS $LINENO ${PIPESTATUS[@]}`; `shift`, `set --`, `source f args`, `jaksh -c 'cmd' name args`, đối số script.
- **Chế độ không tương tác sạch:** không banner/"tạm biệt"; stdin không phải TTY thì chạy như script; script và `.jakshrc` được parse cả file (lỗi cú pháp thì rơi về chạy từng dòng); lỗi một lệnh không dừng script.
- **Builtin:** `read` (`-r -p -s -t -n -d`, Ctrl-C ngắt được), `command [-v|-V]`, `type [-t -p -a]`, `builtin`, `hash`, `echo -e/-E/-n`, `kill -l / -s`.
- **`set`:** `-e`, `-u`, `-x`, `-a`, `-C`, `-o pipefail`, `-o`/`+o`; `set` không đối số in biến.
- **History:** lọc secret khi lưu và `history --scrub`.

**Còn lại:** Đợt 2 trở đi, theo bảng dưới. Các mục đã xong ở trên vẫn để nguyên trong bảng để đối chiếu.

## 1. Bảng gap

Mức độ: **P0** = làm hỏng lệnh hằng ngày hoặc script phổ biến · **P1** = hay dùng · **P2** = ít dùng.
Công sức: **S** < ½ ngày · **M** 1–3 ngày · **L** > 3 ngày hoặc cần đổi kiến trúc.

### 1.1 Tín hiệu & job control

| Tính năng | bash | jaksh (output thật) | Mức | Công | Vị trí |
|---|---|---|---|---|---|
| Ctrl-C khi lệnh đang chạy | Chỉ giết lệnh con, shell sống | **Shell chết** (`waitpid` status=2, SIGINT) | **P0** | S (giảm thiểu) / L (đầy đủ) | `main.rs` REPL, `executor.rs::spawn_external/spawn_pipeline` (không có `setpgid`, không bỏ qua SIGINT) |
| Ctrl-Z → `jobs` / `fg` / `bg` | `[1]+ Stopped sleep 3` | Không dừng được job, `jobs` rỗng | P1 | L | như trên + `builtins::fg/bg` (chỉ gửi SIGCONT, không `tcsetpgrp`) |
| `cmd & cmd2` | Chạy nền rồi tiếp | `token không mong đợi: Word([Literal("wait")])` | **P0** | S | `parser.rs::parse_pipeline`: `&` không được coi là dấu phân tách |
| `wait`, `disown`, `kill %1`, `jobs -l`, `%%` | Có | Chưa có / không parse được | P1 | M | `builtins/mod.rs` |
| Thông báo khi job nền xong | `[1]+ Done …` | Không có | P1 | S | `shell.rs::reap_jobs` |
| `trap '…' EXIT INT ERR` | Có | `không tìm thấy lệnh: trap` | P1 | M | mới |
| `-c 'sleep 1 &'` | Im lặng (không tương tác) | In `[1] 928028` | P2 | S | `executor.rs` (chỉ in khi interactive) |

### 1.2 Gọi shell & chạy script

| Tính năng | bash | jaksh | Mức | Công | Vị trí |
|---|---|---|---|---|---|
| `$?` giữa các lệnh cùng dòng | `false; echo $?` → `1` | → `0` (chỉ REPL cập nhật sau cả dòng) | **P0** | S | `executor.rs::execute`: không gọi `set_last_status` sau mỗi pipeline |
| Tham số script `$1..$9 $# $@ $*`, `shift`, `set --` | `args: A B 2` | `args:   ` (bị bỏ qua: `run_script(_args)`) | **P0** | M | `main.rs::run_script`, `expand.rs` |
| Script đọc cả file | Parse cả file | Chạy **từng dòng**, nên chuỗi/khối nhiều dòng hỏng (`nháy kép không đóng`) | **P0** | M | `main.rs::run_script`, `config::run_script` |
| `echo 'cmd' \| jaksh` (stdin không phải TTY) | Chạy như script | Vào REPL: in banner, timing, "tạm biệt" ra stdout | **P0** | S | `main.rs` (kiểm tra `isatty(0)`) |
| `jaksh -c 'exit 3'` | Không in gì | In `tạm biệt 👋` ra stdout | **P0** | S | builtin `exit` |
| `$0`, `$$`, `$RANDOM`, `$SECONDS`, `$LINENO`, `${PIPESTATUS[@]}` | Có | Rỗng | P1 | S | `shell.rs::get_var` |
| Lỗi cú pháp có số dòng | `line 1: unexpected EOF while looking for matching '"'` | `nháy kép không đóng` (không có dòng/vị trí) | P2 | S | `lexer.rs` |
| Đọc `~/.profile` khi `-l` | Có | `-l` là no-op | P2 | S | `main.rs::parse_cli` |
| Shebang `#!/usr/bin/env jaksh` | — | ✅ chạy được | — | — | — |

### 1.3 Cú pháp điều khiển (cần AST mới)

| Tính năng | bash | jaksh | Mức | Công | Vị trí |
|---|---|---|---|---|---|
| `if/elif/else/fi` | Có | `không tìm thấy lệnh: if` | **P0** | L | `parser.rs`, `executor.rs` |
| `for x in …`, `for ((;;))`, `while`, `until`, `break/continue` | Có | `không tìm thấy lệnh: for` | **P0** | L | như trên |
| `while read l; do …; done` | Có | Không | **P0** | L | như trên + builtin `read` |
| `case … esac` | Có | Không | **P0** | L | như trên |
| Function `f() {…}`, `function f {…}`, `local`, `return` | Có | `không tìm thấy lệnh: f()` | **P0** | L | như trên + scope biến |
| `{ a; b; }` nhóm lệnh | `2` | Chạy `}` như lệnh | P1 | M | như trên |
| `( … )` subshell | Có | `không tìm thấy lệnh: (cd` | P1 | M | như trên (fork) |
| `! cmd` phủ định | `negated` | `không tìm thấy lệnh: !` | **P0** | S | `parser.rs::parse_pipeline` |
| Dòng chưa xong (`if` mở, nháy chưa đóng, `\` cuối) → prompt `PS2` | Chờ nhập tiếp | Báo lỗi ngay | P1 | S | `completion.rs`: thêm rustyline `Validator` |

### 1.4 Expansion

| Tính năng | bash | jaksh | Mức | Công | Vị trí |
|---|---|---|---|---|---|
| `${v:-x} ${v:=x} ${v:?} ${v:+x}` | `def` | rỗng | **P0** | M | `lexer.rs::read_var`, `expand.rs` |
| `${#v} ${v#p} ${v##p} ${v%p} ${v%%p} ${v/a/b} ${v//a/b} ${v:o:l} ${v^^} ${v,,} ${!v}` | `5`, `c.txt`, `c.tar`… | rỗng | **P0** | M | như trên |
| `$((…))`, `((…))`, `let` | `14 3 1 256` | `không tìm thấy lệnh: (2+3*4)` | **P0** | M | `lexer.rs`, mới: bộ tính số học |
| Brace `{a,b}.txt`, `{1..5}` | `a.txt b.txt c.txt` | Giữ nguyên `{a,b,c}.txt` | P1 | M | `expand.rs` (phải chạy trước tilde/glob) |
| `$'a\tb'` | Tab thật | `a\tb` | P1 | S | `lexer.rs` |
| `<( … )`, `>( … )` | `from-procsub` | `không mở được (echo` | P1 | M | `lexer.rs`, `executor.rs` (pipe + `/dev/fd/N`) |
| Tilde trong phép gán `p=~/x` | `/home/u/x` | `~/x` | P1 | S | `executor.rs::parse_assignment` |
| Word splitting theo `$IFS` | Có | Không kiểm tra được vì thiếu `for`; `IFS` chưa được đọc | P1 | M | `expand.rs` |
| `shopt -s nullglob/dotglob/globstar/extglob` | Có | `không tìm thấy lệnh: shopt` (`**` đã hoạt động mặc định) | P2 | S | `expand.rs` |

### 1.5 Redirect

| Tính năng | bash | jaksh | Mức | Công | Vị trí |
|---|---|---|---|---|---|
| `2>&1`, `>&2`, `1>&2`, `n>&m` | `1` | `thiếu đích cho '2>'` | **P0** | M | `lexer.rs` (token), `parser.rs::RedirectKind`, `executor.rs::apply_redirects`, `builtins::RedirectGuard` |
| `cmd > f 2>&1` (thứ tự redirect) | Đúng thứ tự | Không parse được | **P0** | M | như trên |
| Here-doc `<<EOF`, `<<'EOF'`, `<<-EOF` | `line 2` | `thiếu đích cho '<'` | **P0** | M | `lexer.rs` (đọc thân sau dòng lệnh), `executor.rs` |
| Here-string `<<<` | `here string` | `thiếu đích cho '<'` | P1 | S | như trên |
| `&>>` | Có | `thiếu đích cho '&>'` | P1 | S | `lexer.rs` |
| `0< f`, `3> f`, `exec 3>f`, `>&-` | Có | `cat: 0: No such file` | P2 | M | như trên + `exec` không lệnh |
| `set -o noclobber` / `>\|` | `cannot overwrite existing file` | Ghi đè | P2 | S | `apply_redirects` |

### 1.6 Biến & môi trường

| Tính năng | bash | jaksh | Mức | Công | Vị trí |
|---|---|---|---|---|---|
| Biến shell **không** tự export | `x=1; sh -c 'echo $x'` → rỗng | → `1` (mọi biến đều vào env của con) | **P0** | M | `shell.rs::set_var` gọi `std::env::set_var` |
| `export -n`, `readonly`, `declare/typeset` (`-i -x -r -a -A`) | Có | `không tìm thấy lệnh: readonly/declare` | P1 | M | bảng biến mới có cờ |
| Mảng `a=(x y)`, `${a[1]}`, `${#a[@]}`, `a+=(…)`; mảng kết hợp | `y 3 x y z` | `không tìm thấy lệnh: y` | P1 | L | `lexer.rs`, `shell.rs`, `expand.rs` |
| `x+=b` | `ab` | `không tìm thấy lệnh: x+=b` | P1 | S | `executor.rs::parse_assignment` |

### 1.7 Test

| Tính năng | bash | jaksh | Mức | Công | Vị trí |
|---|---|---|---|---|---|
| `[[ … ]]` với `==` glob, `=~` + `BASH_REMATCH`, `&&`/`\|\|` bên trong | Có | `không tìm thấy lệnh: [[` | **P0** | M | parser + builtin mới |
| `test`, `[ ]` | ✅ (dùng `/usr/bin/[`) | — | — | — | — |

### 1.8 Builtin

| Tính năng | bash | jaksh | Mức | Công | Vị trí |
|---|---|---|---|---|---|
| `read` (`-r -p -s -t -a`) | Có | Không có builtin (không có binary ngoài) | **P0** | S | `builtins/mod.rs` |
| `command -v`, `type`, `builtin` | `/usr/bin/sh` | `không tìm thấy lệnh: command` (`install.sh` dùng `command -v`) | **P0** | S | `builtins/mod.rs` |
| `echo -e`, `-E`, cụm `-ne` | Tab thật | `-e a\tb` | P1 | S | `builtins::echo` |
| `source f arg1` (tham số) | `sourced arg1` | `sourced ` | P1 | S | `builtins::source` |
| `pushd/popd/dirs` | Có | Không có | P2 | S | mới |
| `CDPATH` | `/usr` | `cd …/usr: No such file` | P2 | S | `builtins::cd` |
| `umask`, `ulimit`, `times` | Có | Không có | P2 | S | mới (`nix`) |
| `getopts`, `mapfile/readarray` | Có | Không có (`getopts` cần `while`) | P2 | S | mới |
| `printf` builtin (`-v var`) | `00042` | Rơi về `/usr/bin/printf`, không có `-v` | P2 | S | mới |
| `kill -l [N]` | `KILL` | In rỗng | P2 | S | `builtins::kill` |
| `exit` trong function / script | Thoát với mã đúng | Phụ thuộc function | P0 (đi cùng function) | S | — |
| Builtin trạng thái trong pipeline (`cd / \| true`) | Chạy trong subshell, không ảnh hưởng | `không tìm thấy lệnh: cd` | P2 | S | `executor.rs::spawn_pipeline` (cần fork) |

### 1.9 Tuỳ chọn shell

| Tính năng | bash | jaksh | Mức | Công | Vị trí |
|---|---|---|---|---|---|
| `set -e` | Dừng ở lệnh lỗi | Chạy tiếp (`set` là no-op) | **P0** | M | `executor.rs` (cần `$?` đúng trước) |
| `set -u` | `unbound variable` | Chạy tiếp | **P0** | S | `expand.rs` |
| `set -o pipefail` | `1` | `0` | **P0** | S | `spawn_pipeline` |
| `set -x` | In `+ echo traced` ra stderr | Không in | P1 | S | `executor.rs` |
| `set -a` | Export tự động | Không có (hiện "đúng" chỉ vì mọi biến đều bị export) | P1 | S | — |

### 1.10 Tương tác

| Tính năng | bash | jaksh | Mức | Công | Vị trí |
|---|---|---|---|---|---|
| Ctrl-A/E/W/U/K/Y, Alt-B/F, Alt-. | Có | ✅ (rustyline, đã thử Ctrl-W, Ctrl-U, Alt-.) | — | — | — |
| Ctrl-X Ctrl-E mở `$EDITOR` | Có | Không có | P1 | S | `main.rs`: `rl.bind_sequence` + mở editor |
| Chế độ vi (`set -o vi`), `~/.inputrc` | Có | Không có (rustyline hỗ trợ `EditMode::Vi`) | P2 | S | `main.rs` Config + `[ui] edit_mode` |
| Completion `$VAR`, `~user`, lệnh trong `$PATH` | Có | Chỉ có builtin/alias/jak/file; chưa thấy nhánh `$VAR` trong code | P1 | S | `completion.rs::complete` |
| Completion tuỳ biến (`complete -F`) | Có | Có sẵn git/docker cứng trong code, chưa mở cho người dùng | P2 | L | `completion.rs` |
| `PS1`/`PROMPT_COMMAND` | Có | Dùng `prompt` template riêng trong toml | P2 (khác có chủ ý) | S | `prompt.rs` |
| History: timestamp, `history -d`, `fc`, `HISTIGNORE`, chia sẻ giữa nhiều phiên đang mở | Có | Không có | P1 | M | `main.rs`, `history.rs` |
| Nhãn `[1]+ Done` khi job nền kết thúc | Có | Không có | P1 | S | — |

## 2. Những gì jaksh đã làm đúng (đừng sửa nhầm)

Khớp bash trong bộ test:
- **Cơ bản:** `-c` nhiều dòng, comment cuối dòng, `&&`/`||` nối nhiều tầng, `;` cuối dòng.
- **Biến:** `$var`, `${var}`, `VAR=x cmd` (gán tạm), `export`, `unset`, `PWD`/`OLDPWD`.
- **Quoting:** nháy đơn/kép, `\$`, ghép `"a"'b'c`, đối số rỗng `""`, chuỗi nhiều dòng trong nháy (ở chế độ `-c` và REPL).
- **Glob:** `*`, `?`, `[ab]`, không khớp thì giữ nguyên, `.h*`, `**`.
- **Command substitution:** `$(…)` lồng nhau, backtick.
- **Redirect:** `>`, `>>`, `<`, `&>`, `echo hi>o.txt` (không cách).
- **Builtin:** `cd -`, `cd` không đối số, `.`, `eval`, `:`, `which`, `pwd`/`echo` trong pipeline, `test` / `[ ]` (qua binary ngoài).
- **Khác:** shebang `#!/usr/bin/env jaksh`, output lớn (`seq 1 100000`), UTF-8 khi in, thời gian khởi động tương đương bash (~2.5 ms/lần).

**Lưu ý: vài case "pass" là pass giả**, cần kiểm tra lại khi sửa:
- `set-x`: bộ test không so stderr.
- `hash`: `$?` luôn là 0.
- `dotenv`: chỉ khớp vì mọi biến đều bị export.
- `shopt-dotglob`: `shopt` không tồn tại, nhưng glob `.hid*` vẫn khớp file ẩn.

## 3. Khác bash có chủ ý

| Hành vi | Lý do | Cách tắt |
|---|---|---|
| `curl` ra terminal được format (dòng HTTP status + JSON đẹp) | Dễ đọc API | `[ui] pretty_curl = false`, hoặc pipe/redirect, hoặc dùng `-o/-O/-N/-v/-w` |
| Cờ `--jak` trên `ls/ps/df/du/git` | Output đẹp | Không dùng cờ |
| `!prefix` chỉ mở rộng ở đầu từ (`echo hi!there` giữ nguyên) | Tránh lỗi "event not found" bất ngờ | — |
| Alias hoạt động cả trong `-c` và script (bash tắt alias khi không tương tác) | Thân thiện | **Nên xem lại**: làm script chạy khác nhau giữa bash và jaksh |
| Timing `⏱`, gợi ý exit code, did-you-mean, banner | Trải nghiệm tương tác | `[timing]`, `[greeting]` trong `~/.jakshrc.toml` |
| Paste nhiều dòng: mỗi dòng là một lệnh | Giống bash | — |
| Prompt cấu hình bằng `prompt = "…"` trong toml thay vì `PS1` | Đơn giản hơn | — |

## 4. Lộ trình theo đợt

### Đợt 1: P0 rẻ, không đổi kiến trúc (≈ 3–4 ngày)
1. **Ctrl-C/Ctrl-Z không giết hay dừng shell:** shell bỏ qua `SIGINT/SIGQUIT/SIGTSTP/SIGTTOU`; tiến trình con khôi phục mặc định trong `pre_exec`. Đây là bản giảm thiểu; job control đầy đủ để Đợt 3.
2. `$?` cập nhật sau mỗi pipeline trong `execute`.
3. `&` là dấu phân tách lệnh; `! pipeline`.
4. Tách **biến shell** khỏi **env**: bảng `vars: HashMap<String, Var { value, exported, readonly }>`, con chỉ nhận biến đã export. Kèm `export -n`, `readonly`, `x+=`.
5. Chế độ không tương tác sạch: không banner/"tạm biệt" khi `-c`, script hay stdin không phải TTY; stdin không phải TTY thì chạy như script.
6. Biến đặc biệt `$0 $$ $RANDOM $SECONDS $LINENO`; tham số vị trí `$1… $# $@ $*`, `shift`, `set --`, `source f args`.
7. Redirect fd: `2>&1`, `>&2`, `n>&m`, `&>>`, `n<`, `>&-`. Here-doc, here-string.
8. Builtin: `read`, `command`, `type`, `builtin`, `echo -e/-E`, `kill -l`.
9. `set -u`, `set -o pipefail`, `set -x`.
10. **Chặn lưu secret vào history** (xem mục 5): đã xảy ra thật trong lịch sử của chủ dự án.

### Đợt 2: Parser & executor mới (≈ 2–3 tuần), mở khoá script bash
- **AST:** `enum Command { Simple, Pipeline, If, For, ForArith, While, Until, Case, FuncDef, Group, Subshell, DBracket, Arith }`. Lexer nhận diện từ khoá theo ngữ cảnh (chỉ ở vị trí lệnh) và đọc thân here-doc.
- **Executor:** đệ quy trên AST. `break/continue/return` truyền bằng enum điều khiển, không dùng `Err`. Function lưu AST; `local` dùng stack scope.
- **Expansion theo đúng thứ tự bash:** brace → tilde → `${…}` / `$((…))` / `$(…)` → word splitting theo `IFS` → glob → bỏ nháy. Hiện mọi thứ dồn trong `build_argv`/`expand_word`, cần tách.
- `[[ … ]]` (glob, `=~` + `BASH_REMATCH`), `$((…))` / `((…))` / `let`.
- `set -e` (đúng ngữ nghĩa ngoại lệ trong `if`/`&&`/`||`).
- Script: parse **cả file** thay vì từng dòng (cùng một parser cho `~/.jakshrc`, `source`, `-c`).
- REPL: rustyline `Validator` để dòng chưa xong chuyển sang prompt tiếp (`PS2`) thay vì báo lỗi.
- **Cân nhắc trước khi tự viết:** đánh giá crate `brush-parser` (parser bash viết bằng Rust của dự án brush) để lấy AST, giữ executor/UX riêng của jaksh. Có thể tiết kiệm phần lớn công sức Đợt 2; cần kiểm tra license và độ ổn định.

### Đợt 3: Job control thật (≈ 1 tuần)
- `setpgid` cho mỗi pipeline, `tcsetpgrp` trao/thu terminal, `waitpid(WUNTRACED)` để bắt Ctrl-Z.
- `fg`/`bg`/`jobs -l`/`%N %% %-`/`wait`/`disown`, thông báo `Done`.
- `trap` (EXIT, INT, ERR, …).

### Đợt 4: P1/P2 còn lại (≈ 1–2 tuần)
- Mảng thường và mảng kết hợp, `declare`.
- Brace expansion, `$'…'`, process substitution.
- `pushd/popd`, `CDPATH`, `umask`, `ulimit`, `getopts`, `mapfile`, `printf` builtin, `shopt`, `noclobber`.
- History: timestamp, `history -d`, `fc`, chia sẻ giữa các phiên.
- Ctrl-X Ctrl-E, chế độ vi, completion `$VAR`.
- Xem lại việc alias chạy trong script.

### Đợt 5: Vượt bash (xem mục 5)

## 5. Đề xuất vượt bash (không phá tương thích)

| # | Tính năng | Giá trị | Công |
|---|---|---|---|
| 1 | **Chặn secret khỏi history**: phát hiện `ghp_…`, `github_pat_…`, `sk-…`, `AKIA…`, `Authorization:`, `X-Api-Key:`, `user:pass@` trong URL. Không lưu, hoặc lưu bản đã che `***`, và cảnh báo một dòng | Chống lộ token: đã xảy ra thật | S |
| 2 | **Tô màu cú pháp khi gõ** (rustyline `Highlighter`): lệnh không tồn tại tô đỏ, chuỗi, biến, toán tử | Bắt lỗi trước khi Enter, giống fish | M |
| 3 | **Prompt nhiều dòng thông minh**: nháy chưa đóng hoặc `if` mở thì hiện gợi ý đang thiếu gì | Thay lỗi khó hiểu | S (đi cùng Validator) |
| 4 | **Ctrl-R kiểu fuzzy** + ưu tiên lệnh từng chạy trong thư mục hiện tại | Tìm lệnh nhanh hơn | M |
| 5 | **History chia sẻ thời gian thực** giữa các tab, có timestamp, cwd và exit code; `history --failed`, `history --here` | Mạnh hơn `HISTTIMEFORMAT` | M |
| 6 | **Cảnh báo lệnh nguy hiểm** (`rm -rf /`, `rm -rf ~`, `chmod -R 777 /`, `git push -f` lên `main`) | An toàn cho người mới | S |
| 7 | **Kiểm tra script**: `jaksh --check script.sh` báo cú pháp mà jaksh chưa hỗ trợ, kèm số dòng | Minh bạch trong lúc chưa đủ tương thích | S (sau Đợt 2) |

## 6. Bộ test so sánh

- **Vị trí:** [`tests/bash-compat/`](../tests/bash-compat/)
  - `cases.py`: 160 case, chia nhóm `invoke`, `control`, `expand`, `redirect`, `vars`, `test`, `builtin`, `options`, `jobs`, `compat`.
  - `run.py`: chạy so sánh.
  - `signals.py`, `nested.py`: Ctrl-C / Ctrl-Z / job control qua pty, và jaksh chạy lồng trong bash.
- **Chạy:**
  ```bash
  cargo build
  python3 tests/bash-compat/run.py target/debug/jaksh                     # tóm tắt + diff các case lỗi
  python3 tests/bash-compat/run.py target/debug/jaksh --only if-basic,heredoc -v
  python3 tests/bash-compat/signals.py target/debug/jaksh             # Ctrl-C/Ctrl-Z/job control qua pty
  ```
- **Giới hạn:**
  - Chỉ so stdout + exit code; stderr chỉ xem tay.
  - Phần tương tác đánh giá theo heuristic.
  - Mỗi case dùng HOME tạm, nên không đụng tới `~/.config/jaksh` thật.
- **Quy tắc:** mỗi tính năng sửa xong phải chuyển case tương ứng sang PASS; `signals.py` phải luôn 100% PASS. Nên chạy trong CI.
