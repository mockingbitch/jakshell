#!/usr/bin/env bash
# JakShell bootstrap — clone repo + chạy install.sh.
#
# Dùng:
#   curl -fsSL https://raw.githubusercontent.com/mockingbitch/jakshell/master/bootstrap.sh | bash
#
# Biến môi trường tuỳ chỉnh:
#   JAKSH_DIR    nơi clone repo (default: ~/.jakshell)
#   JAKSH_REPO   URL repo (default: https://github.com/mockingbitch/jakshell.git)
#   JAKSH_REF    branch/tag cần checkout (default: master)
#   PREFIX       nơi cài binary (default: ~/.local/bin) — pass tiếp cho install.sh

set -eu

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

has() { command -v "$1" >/dev/null 2>&1; }

JAKSH_DIR="${JAKSH_DIR:-$HOME/.jakshell}"
JAKSH_REPO="${JAKSH_REPO:-https://github.com/mockingbitch/jakshell.git}"
JAKSH_REF="${JAKSH_REF:-master}"

msg "JakShell bootstrap"
msg "  repo: ${B}${JAKSH_REPO}${X}  ${D}(${JAKSH_REF})${X}"
msg "  dir:  ${B}${JAKSH_DIR}${X}"

# ─── 1) git phải có sẵn ───────────────────────────────────────────────────────
if ! has git; then
    fail "Cần ${B}git${X} để clone source. Cài git rồi chạy lại."
    case "$(uname -s)" in
        Darwin) echo "  macOS: xcode-select --install   hoặc   brew install git" ;;
        Linux)  echo "  Debian/Ubuntu: sudo apt-get install -y git"
                echo "  Fedora/RHEL:   sudo dnf install -y git"
                echo "  Arch:          sudo pacman -S git" ;;
    esac
    exit 1
fi

# ─── 2) Clone hoặc cập nhật repo ──────────────────────────────────────────────
if [ -d "$JAKSH_DIR/.git" ]; then
    msg "Repo đã tồn tại — cập nhật bằng git pull…"
    git -C "$JAKSH_DIR" fetch --tags --quiet origin
    git -C "$JAKSH_DIR" checkout --quiet "$JAKSH_REF" 2>/dev/null || true
    git -C "$JAKSH_DIR" pull --rebase --quiet || warn "git pull có lỗi — vẫn tiếp tục với code hiện có."
    ok "Đã cập nhật ${B}${JAKSH_DIR}${X}"
elif [ -e "$JAKSH_DIR" ]; then
    fail "${JAKSH_DIR} đã tồn tại nhưng KHÔNG phải git repo."
    echo "  Đặt biến JAKSH_DIR để dùng đường dẫn khác:"
    echo "    JAKSH_DIR=\$HOME/somewhere/jakshell  curl -fsSL .../bootstrap.sh | bash"
    exit 1
else
    msg "Đang clone vào ${B}${JAKSH_DIR}${X}…"
    git clone --depth 1 --branch "$JAKSH_REF" "$JAKSH_REPO" "$JAKSH_DIR"
    ok "Đã clone"
fi

# ─── 3) Chạy install.sh trong repo ────────────────────────────────────────────
if [ ! -x "$JAKSH_DIR/install.sh" ]; then
    fail "Không thấy install.sh trong $JAKSH_DIR"
    exit 1
fi

msg "Chạy install.sh --yes…"
# Khi pipe qua bash (stdin không phải tty), tự bật --yes để không bị treo prompt.
INSTALL_ARGS="--yes"
[ -n "${PREFIX:-}" ] && INSTALL_ARGS="$INSTALL_ARGS --prefix $PREFIX"

# shellcheck disable=SC2086
"$JAKSH_DIR/install.sh" $INSTALL_ARGS

echo
ok "${B}Bootstrap xong!${X}"
echo
printf "Source repo:    ${C}%s${X}\n" "$JAKSH_DIR"
printf "Cập nhật sau:   ${C}jak self-update${X}   ${D}(hoặc: cd %s && git pull && ./install.sh)${X}\n" "$JAKSH_DIR"
