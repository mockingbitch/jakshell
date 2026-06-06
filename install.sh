#!/usr/bin/env bash
# JakShell installer — cài Rust + dependency cần thiết, build, copy binary.
#
# Cờ:
#   --yes / -y     không hỏi, mặc định đồng ý mọi prompt
#   --no-deps      bỏ qua cài runtime deps (chỉ cần Rust để build)
#   --prefix PATH  đổi nơi cài binary (default: ~/.local/bin)
#
# Dùng:
#   ./install.sh
#   ./install.sh --yes
#   PREFIX=/usr/local/bin ./install.sh

set -eu

# ─── colors ───────────────────────────────────────────────────────────────────
if [ -t 1 ]; then
    G="\033[32m"; Y="\033[33m"; R="\033[31m"; C="\033[36m"
    D="\033[2m";  B="\033[1m";  X="\033[0m"
else
    G=""; Y=""; R=""; C=""; D=""; B=""; X=""
fi

msg()  { printf "${C}▸${X} %b\n" "$*"; }
ok()   { printf "${G}✓${X} %b\n" "$*"; }
warn() { printf "${Y}⚠${X} %b\n" "$*" >&2; }
fail() { printf "${R}✗${X} %b\n" "$*" >&2; }

# ─── flags ────────────────────────────────────────────────────────────────────
ASSUME_YES=0
NO_DEPS=0
PREFIX="${PREFIX:-$HOME/.local/bin}"

while [ "$#" -gt 0 ]; do
    case "$1" in
        -y|--yes)    ASSUME_YES=1 ;;
        --no-deps)   NO_DEPS=1 ;;
        --prefix)    PREFIX="$2"; shift ;;
        --prefix=*)  PREFIX="${1#--prefix=}" ;;
        -h|--help)
            sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) warn "tham số không rõ: $1" ;;
    esac
    shift
done

ask_yes() {
    if [ "$ASSUME_YES" = "1" ]; then return 0; fi
    local prompt="$1"
    local reply
    printf "${B}?${X} %s [Y/n] " "$prompt"
    read -r reply || true
    case "$reply" in
        n|N|no|No|NO) return 1 ;;
        *) return 0 ;;
    esac
}

has() { command -v "$1" >/dev/null 2>&1; }

# ─── 1) Detect OS + package manager ───────────────────────────────────────────
OS="unknown"
PKG_MGR=""
INSTALL_PKG=""

case "$(uname -s)" in
    Darwin)
        OS="macos"
        if has brew; then
            PKG_MGR="brew"
            INSTALL_PKG="brew install"
        fi
        ;;
    Linux)
        OS="linux"
        if   has apt-get;  then PKG_MGR="apt";    INSTALL_PKG="sudo apt-get install -y"
        elif has dnf;      then PKG_MGR="dnf";    INSTALL_PKG="sudo dnf install -y"
        elif has pacman;   then PKG_MGR="pacman"; INSTALL_PKG="sudo pacman -S --noconfirm"
        elif has zypper;   then PKG_MGR="zypper"; INSTALL_PKG="sudo zypper --non-interactive install"
        elif has apk;      then PKG_MGR="apk";    INSTALL_PKG="sudo apk add"
        fi
        ;;
    *) OS="other" ;;
esac

msg "OS: ${B}${OS}${X}  ${D}package manager:${X} ${B}${PKG_MGR:-(không có)}${X}"

# ─── 2) Load cargo env nếu rustup đã cài ──────────────────────────────────────
if ! has cargo; then
    [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env" 2>/dev/null || true
    [ -x "$HOME/.cargo/bin/cargo" ] && export PATH="$HOME/.cargo/bin:$PATH"
fi

# ─── 3) Cài Rust nếu thiếu ────────────────────────────────────────────────────
install_rust() {
    msg "Đang cài Rust (rustup)…"
    if ! has curl; then
        warn "Cần curl để tải rustup."
        return 1
    fi
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- \
        -y --default-toolchain stable --profile default
    . "$HOME/.cargo/env"
    ok "Rust đã cài: $(rustc --version)"
}

if ! has cargo; then
    warn "cargo chưa có trên PATH."
    if ask_yes "Cài Rust toolchain qua rustup.rs?"; then
        install_rust
    else
        fail "Cần Rust để build JakShell. Cài thủ công tại https://rustup.rs"
        exit 1
    fi
fi
ok "cargo: $(command -v cargo)"

# ─── 4) Runtime deps ──────────────────────────────────────────────────────────
# (name, package_name_apt, package_name_dnf, package_name_pacman, package_name_brew, ghi chú)
declare -a REQUIRED=("git" "curl" "tar")
declare -a OPTIONAL=("ripgrep")

# Linux cần xdg-utils cho `jak open` xử lý file
if [ "$OS" = "linux" ]; then
    OPTIONAL+=("xdg-utils")
fi

# Map từ "common name" → tên gói theo từng pkg manager
pkg_name() {
    local name="$1"
    local mgr="$2"
    case "$name" in
        ripgrep)
            case "$mgr" in
                brew) echo "ripgrep" ;;
                apt)  echo "ripgrep" ;;
                dnf)  echo "ripgrep" ;;
                pacman) echo "ripgrep" ;;
                zypper) echo "ripgrep" ;;
                apk)  echo "ripgrep" ;;
            esac
            ;;
        xdg-utils)
            case "$mgr" in
                apt)  echo "xdg-utils" ;;
                dnf)  echo "xdg-utils" ;;
                pacman) echo "xdg-utils" ;;
                zypper) echo "xdg-utils" ;;
                apk)  echo "xdg-utils" ;;
                *) echo "" ;;
            esac
            ;;
        *)
            echo "$name"
            ;;
    esac
}

# Map check command (vd cài "ripgrep" gói nhưng binary là "rg")
check_bin() {
    case "$1" in
        ripgrep) echo "rg" ;;
        xdg-utils) echo "xdg-open" ;;
        *) echo "$1" ;;
    esac
}

install_pkgs() {
    if [ -z "$PKG_MGR" ]; then
        warn "Không có package manager — bỏ qua cài deps tự động."
        return
    fi
    local list=("$@")
    if [ "${#list[@]}" -eq 0 ]; then return; fi
    msg "Cài: ${B}${list[*]}${X}  ${D}qua ${PKG_MGR}${X}"
    # shellcheck disable=SC2086
    $INSTALL_PKG "${list[@]}" || warn "Cài gói có lỗi (xem log phía trên)."
}

if [ "$NO_DEPS" = "0" ]; then
    msg "Kiểm tra runtime dependency…"

    MISSING_REQ=()
    MISSING_OPT=()

    for dep in "${REQUIRED[@]}"; do
        bin=$(check_bin "$dep")
        if has "$bin"; then
            ok "$dep ($bin)"
        else
            warn "thiếu: $dep ($bin)"
            MISSING_REQ+=("$dep")
        fi
    done

    for dep in "${OPTIONAL[@]}"; do
        bin=$(check_bin "$dep")
        if has "$bin"; then
            ok "$dep ($bin)"
        else
            warn "thiếu (tuỳ chọn): $dep ($bin)"
            MISSING_OPT+=("$dep")
        fi
    done

    ALL_MISSING=()
    for d in "${MISSING_REQ[@]:-}"; do [ -n "${d:-}" ] && ALL_MISSING+=("$d"); done
    for d in "${MISSING_OPT[@]:-}"; do [ -n "${d:-}" ] && ALL_MISSING+=("$d"); done

    if [ "${#ALL_MISSING[@]}" -gt 0 ]; then
        if [ -z "$PKG_MGR" ]; then
            if [ "$OS" = "macos" ]; then
                warn "Chưa có Homebrew — cài tại https://brew.sh rồi chạy lại."
                warn "Hoặc tự cài: ${ALL_MISSING[*]}"
            else
                warn "Không phát hiện package manager. Tự cài: ${ALL_MISSING[*]}"
            fi
        else
            pkg_list=()
            for d in "${ALL_MISSING[@]}"; do
                p=$(pkg_name "$d" "$PKG_MGR")
                [ -n "$p" ] && pkg_list+=("$p")
            done
            if [ "${#pkg_list[@]}" -gt 0 ]; then
                if ask_yes "Cài qua ${PKG_MGR}: ${pkg_list[*]} ?"; then
                    install_pkgs "${pkg_list[@]}"
                else
                    warn "Bỏ qua cài deps."
                fi
            fi
        fi
    fi
fi

# ─── 5) Build ─────────────────────────────────────────────────────────────────
msg "Build (release)…"
cargo build --release

BIN="target/release/jaksh"
if [ ! -x "$BIN" ]; then
    fail "Không thấy binary tại $BIN — build thất bại."
    exit 1
fi

# ─── 6) Cài binary ────────────────────────────────────────────────────────────
mkdir -p "$PREFIX"
cp -f "$BIN" "$PREFIX/jaksh"
ok "Đã cài: ${B}$PREFIX/jaksh${X}"

# ─── 7) Tạo sample config nếu chưa có ─────────────────────────────────────────
if [ ! -f "$HOME/.jakshrc.toml" ] && [ -f "examples/jakshrc.toml" ]; then
    cp "examples/jakshrc.toml" "$HOME/.jakshrc.toml"
    ok "Tạo ${B}~/.jakshrc.toml${X} (cấu hình mẫu)"
fi
if [ ! -f "$HOME/.jakshrc" ] && [ -f "examples/jakshrc" ]; then
    cp "examples/jakshrc" "$HOME/.jakshrc"
    ok "Tạo ${B}~/.jakshrc${X} (script khởi động mẫu)"
fi

# ─── 8) PATH check ────────────────────────────────────────────────────────────
case ":$PATH:" in
    *":$PREFIX:"*) ;;
    *)
        warn "$PREFIX chưa có trong PATH. Thêm dòng sau vào ~/.bashrc / ~/.zshrc:"
        printf "    ${C}export PATH=\"%s:\$PATH\"${X}\n" "$PREFIX"
        ;;
esac

# ─── 9) Gợi ý sử dụng ─────────────────────────────────────────────────────────
echo
ok "${B}JakShell đã sẵn sàng!${X}"
echo
printf "Chạy thử:        ${C}%s/jaksh${X}\n" "$PREFIX"
printf "Đặt làm shell:   ${D}echo %s/jaksh | sudo tee -a /etc/shells${X}\n" "$PREFIX"
printf "                 ${D}chsh -s %s/jaksh${X}\n" "$PREFIX"
echo
