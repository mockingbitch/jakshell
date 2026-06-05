# JakShell

Shell viết bằng Rust cho macOS và Linux — nhanh, gọn, thân thiện.

## Tính năng

- Cú pháp giống POSIX: `|`, `&&`, `||`, `;`, `&`, `>`, `>>`, `<`, `2>`, `2>>`, `&>`
- Biến: `$VAR`, `${VAR}`, `$?`; tilde `~`; glob `* ? [abc]`
- Nháy đơn `'...'` (literal) và nháy kép `"..."` (cho phép `$VAR`)
- Job nền & quản lý job: `&`, `jobs`, `fg`, `bg`, `kill`
- Alias, export, history, source/`.`
- Prompt tuỳ biến (màu sắc, nhánh git, mã thoát)
- Theme dựng sẵn: `ocean`, `forest`, `sunset`, `mono`, `default`
- Tab completion cho lệnh / alias / builtin / đường dẫn
- Gợi ý lệnh đúng khi gõ sai (did-you-mean)
- Lệnh tiện ích `jak …` cho người dùng phổ thông
- Builtin `explain <lệnh>` — chạy lệnh kèm chú thích các cột output bằng tiếng Việt (giữ nguyên thuật ngữ tiếng Anh)

## Cài đặt

Cần Rust (rustup.rs).

```bash
./install.sh
```

Lệnh trên sẽ build release và copy `jaksh` vào `~/.local/bin`. Đảm bảo `~/.local/bin` nằm trong `PATH`.

Chạy thử:

```bash
~/.local/bin/jaksh
```

Đặt làm shell mặc định (tuỳ chọn):

```bash
echo $HOME/.local/bin/jaksh | sudo tee -a /etc/shells
chsh -s $HOME/.local/bin/jaksh
```

## Cấu hình

- `~/.jakshrc.toml` — theme, prompt, alias, env (xem `examples/jakshrc.toml`)
- `~/.jakshrc` — script khởi động, chạy lệnh shell mỗi khi mở

## Lệnh tiện ích `jak`

| Lệnh                    | Mô tả                                       |
|-------------------------|----------------------------------------------|
| `jak clean [--dry]`     | Dọn cache + tệp tạm bạn sở hữu               |
| `jak backup <thư_mục>`  | Nén `.tar.gz` với tên kèm ngày-giờ           |
| `jak update`            | Tự dò brew / apt / dnf / pacman và cập nhật  |
| `jak find <tên>`        | Tìm file/thư mục theo tên                    |
| `jak open <đường_dẫn>`  | Mở bằng app mặc định                         |
| `jak sysinfo`           | OS, CPU, RAM, đĩa                            |
| `jak theme <tên>`       | Đổi theme nhanh                              |
| `jak ip`                | IP nội bộ + public                           |
| `jak weather [tp]`      | Thời tiết qua wttr.in                        |

Gõ `help` hoặc `?` bất kỳ lúc nào để xem trợ giúp.

## `explain` — chú thích kết quả lệnh

```bash
explain                # liệt kê tất cả lệnh có sẵn chú thích
explain list           # tương tự
explain ls -la         # in legend các cột → rồi chạy ls -la
explain ps aux
explain df -h
explain chmod
explain git status
```

Đã có chú thích cho: `ls`, `ps`, `df`, `du`, `free`, `top/htop`, `uptime`, `who/w`,
`netstat`, `ss`, `ifconfig`, `ip`, `chmod`, `stat`, `lsof`, `ping`, `curl`,
`git status / log / diff / branch`.

Triết lý: phần mô tả là tiếng Việt nhưng thuật ngữ chuẩn (PID, owner, permissions,
inode, RSS, MTU, …) giữ nguyên gốc để không gây hiểu nhầm.

## `--jak` — tô màu + reformat output

Thêm cờ `--jak` vào cuối lệnh hỗ trợ để JakShell intercept output và in lại đẹp hơn:

```bash
ls -la --jak     # tô màu permissions từng ký tự (d/rwx), bold thư mục, * cho exec
ps aux --jak     # USER màu, PID cyan, %CPU/%MEM tô theo ngưỡng
df -h --jak      # Use% xanh→vàng→đỏ theo % đầy, size tô theo đơn vị (K/M/G/T)
du -sh --jak     # căn cột size + tô màu
```

- Cờ `--jak` được JakShell **tách ra trước** khi gọi lệnh thật → command thật
  không thấy `--jak`.
- Chỉ hoạt động trên single command (không pipe, không background).
- Không có `--jak` thì lệnh chạy y nguyên — không có biến hoá ngầm.

## `jak find` — tìm kiếm tự nhiên

Tất cả tính năng tìm kiếm thân thiện đều nằm dưới namespace `jak find`. Lệnh `find` của hệ thống KHÔNG bị JakShell intercept — gõ `find . -name ...` vẫn chạy như bình thường.

```bash
jak find Cargo.toml                     # viết tắt: tương đương `jak find file Cargo.toml`
jak find file "*.rs" in src             # glob, giới hạn thư mục
jak find file Cargo.toml in ~/Desktop   # ~ được mở rộng
jak find dir "src"                      # tìm thư mục
jak find text "TODO" in src             # grep (ưu tiên rg nếu có)
jak find big                            # 20 file lớn nhất ở cwd
jak find big in /var/log
jak find recent                         # file sửa trong 24h
jak find empty                          # file rỗng

find . -name "*.rs" -type f             # /usr/bin/find — không can thiệp
```

Quy tắc:
- Glob: `*`, `?`, `[abc]`. Không có ký tự glob → so khớp **substring** không phân biệt hoa thường.
- Từ khoá `in` hoặc `trong` để chỉ thư mục tìm.
- Tự bỏ qua: `.git`, `node_modules`, `target`, `venv`, `__pycache__`, `dist`, `build`, …
- Quote tôn trọng chuẩn POSIX: `"*.rs"` (có nháy) là literal, `*.rs` (không nháy) bị shell glob-expand trước khi vào `jak find`.
