#!/usr/bin/env bash
# Tạo repo mới trên Gitea và đẩy mã nguồn CưDân Hub lên đó.
#
# Biến môi trường:
#   GITEA_URL    (bắt buộc) vd. https://git.congty.vn
#   GITEA_TOKEN  (bắt buộc) Access token có quyền write:repository
#                (Gitea → Settings → Applications → Generate New Token)
#   GITEA_OWNER  (tuỳ chọn) tên tổ chức; bỏ trống = tạo dưới tài khoản của token
#   REPO_NAME    (tuỳ chọn) mặc định: cudan-hub
#   PRIVATE      (tuỳ chọn) true/false, mặc định: true
#   DRY_RUN=1    chỉ chuẩn bị nhánh xuất, không gọi Gitea
#
# Dùng:  GITEA_URL=https://git.example.com GITEA_TOKEN=xxx ./scripts/publish-to-gitea.sh
set -euo pipefail

REPO_NAME="${REPO_NAME:-cudan-hub}"
PRIVATE="${PRIVATE:-true}"
DESCRIPTION="Mini app cho cư dân chung cư: thông tin BQL, đặt hàng nội khu, quảng bá và đánh giá"

project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
git_root="$(git -C "$project_dir" rev-parse --show-toplevel)"

# Nếu dự án đang nằm trong thư mục con của một repo khác, tách lịch sử của
# thư mục đó ra một nhánh riêng (git subtree split) để repo mới chỉ chứa dự án này.
if [[ "$project_dir" == "$git_root" ]]; then
  export_ref="HEAD"
else
  prefix="${project_dir#"$git_root"/}"
  export_branch="export/${REPO_NAME}"
  echo "→ Tách thư mục '${prefix}' thành nhánh ${export_branch}…"
  # Tạo lại nhánh xuất mỗi lần chạy để luôn khớp với HEAD hiện tại.
  git -C "$git_root" branch -D "$export_branch" >/dev/null 2>&1 || true
  git -C "$git_root" subtree split --quiet --prefix="$prefix" -b "$export_branch" >/dev/null
  export_ref="$export_branch"
fi
echo "→ Sẵn sàng đẩy: $(git -C "$git_root" log --oneline -1 "$export_ref")"

if [[ "${DRY_RUN:-}" == "1" ]]; then
  echo "DRY_RUN=1: dừng trước khi gọi Gitea."
  exit 0
fi

: "${GITEA_URL:?Thiếu GITEA_URL}"
: "${GITEA_TOKEN:?Thiếu GITEA_TOKEN}"
GITEA_URL="${GITEA_URL%/}"
auth=(-H "Authorization: token ${GITEA_TOKEN}")

username="$(curl -fsS "${auth[@]}" "${GITEA_URL}/api/v1/user" | sed -n 's/.*"login":"\([^"]*\)".*/\1/p')"
[[ -n "$username" ]] || { echo "Không xác thực được với Gitea — kiểm tra GITEA_URL/GITEA_TOKEN." >&2; exit 1; }
owner="${GITEA_OWNER:-$username}"

if [[ -n "${GITEA_OWNER:-}" ]]; then
  create_url="${GITEA_URL}/api/v1/orgs/${GITEA_OWNER}/repos"
else
  create_url="${GITEA_URL}/api/v1/user/repos"
fi

echo "→ Tạo repo ${owner}/${REPO_NAME} trên ${GITEA_URL}…"
status="$(curl -sS -o /dev/null -w '%{http_code}' "${auth[@]}" -H 'Content-Type: application/json' \
  -X POST "$create_url" \
  -d "{\"name\":\"${REPO_NAME}\",\"description\":\"${DESCRIPTION}\",\"private\":${PRIVATE},\"default_branch\":\"main\"}")"
case "$status" in
  201) echo "  Đã tạo repo." ;;
  409) echo "  Repo đã tồn tại, sẽ đẩy mã lên repo có sẵn." ;;
  *) echo "Tạo repo thất bại (HTTP ${status})." >&2; exit 1 ;;
esac

# Token đi qua header, không ghi vào URL remote hay .git/config.
basic="$(printf '%s:%s' "$username" "$GITEA_TOKEN" | base64 | tr -d '\n')"
remote_url="${GITEA_URL}/${owner}/${REPO_NAME}.git"
echo "→ Đẩy mã lên ${remote_url} (nhánh main)…"
git -C "$git_root" -c http.extraHeader="Authorization: Basic ${basic}" \
  push "$remote_url" "${export_ref}:refs/heads/main"

echo "✓ Xong: ${GITEA_URL}/${owner}/${REPO_NAME}"
