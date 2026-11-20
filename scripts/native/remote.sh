#!/bin/bash
# 把吃 CPU、内存的构建和测试放到另一台 Mac 上跑（2026-11-20）。
# 在仓库所在的这台 Mac 上运行；远端只放同步过去的副本，不碰 git 历史，提交照旧在本机做。
#
#   scripts/native/remote.sh sync        同步仓库和 ../dbx 到远端
#   scripts/native/remote.sh run <命令>  同步后，在远端仓库根目录跑一条 shell 命令
#   scripts/native/remote.sh verify      同步后跑全量验证，打印摘要（日志留在远端 logs/）
#   scripts/native/remote.sh build       同步后远端 pnpm native:build，成功才把 .app 拷回本机
#   scripts/native/remote.sh status      远端磁盘余量与各产物目录大小
#
# 远端由环境变量给出，不写进仓库：
#   AERORIC_REMOTE_HOST            必填，user@host
#   AERORIC_REMOTE_KEY             登录密钥，默认 ~/.ssh/id_ed25519_mini
#   AERORIC_REMOTE_DIR             远端目录（相对远端 home），默认 aeroric-build
#   AERORIC_REMOTE_RUST            远端用的 Rust 版本（rustup），默认取本机 cargo 的版本，
#                                  两边的 rustfmt 结果和编译警告才一致
#   AERORIC_REMOTE_MAX_TARGET_GB   远端 core/target 超过这么大就先 cargo clean，默认 60
#
# native:generate 钉死 XcodeGen 2.46.0：远端放一份在 <远端目录>/tools/xcodegen-2.46.0/
# （从本机 Homebrew 的 Cellar 原样拷过去），PATH 里排在最前。
#
# 远端磁盘有限：编译不留增量缓存（CARGO_INCREMENTAL=0），调试信息只留行号
# （line-tables-only），每次结束报告磁盘余量。只有 macOS 自带的 bash 3.2，别用 bash 4 语法。
set -euo pipefail

host="${AERORIC_REMOTE_HOST:?设置 AERORIC_REMOTE_HOST，例如 user@192.168.1.2}"
key="${AERORIC_REMOTE_KEY:-$HOME/.ssh/id_ed25519_mini}"
dir="${AERORIC_REMOTE_DIR:-aeroric-build}"
max_gb="${AERORIC_REMOTE_MAX_TARGET_GB:-60}"
repo="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
dbx="$(cd -- "$repo/../dbx" && pwd -P)"
rust="${AERORIC_REMOTE_RUST:-$(cargo --version | awk '{print $2}')}"
ssh_opts="-i $key -o BatchMode=yes -o ConnectTimeout=10"
app_rel=".artifacts/native/DerivedData/Build/Products/Debug/AeroricNative.app"

remote() {
  # shellcheck disable=SC2086
  ssh $ssh_opts "$host" "$@"
}

sync() {
  remote "mkdir -p ~/$dir/Aeroric ~/$dir/dbx ~/$dir/logs"
  # git 忽略的都不传：产物、依赖缓存、生成的工程，以及 .claude/worktrees 这类本地目录
  # （有时几十 G）。--delete 不删被排除的路径，远端自己的产物留着。
  local ignored
  ignored="$(mktemp)"
  git -C "$repo" -c core.quotepath=off ls-files --others --ignored --exclude-standard --directory \
    | sed 's#^#/#' > "$ignored"
  rsync -a --delete -e "ssh $ssh_opts" --exclude-from="$ignored" --exclude '.DS_Store' \
    "$repo/" "$host:$dir/Aeroric/"
  # dbx 只要工作区（已打好补丁的那份），不要它的 git 与被忽略的产物。
  git -C "$dbx" -c core.quotepath=off ls-files --others --ignored --exclude-standard --directory \
    | sed 's#^#/#' > "$ignored"
  rsync -a --delete -e "ssh $ssh_opts" --exclude-from="$ignored" --exclude '/.git/' \
    --exclude '.DS_Store' "$dbx/" "$host:$dir/dbx/"
  rm -f "$ignored"
}

# 远端每条命令前的环境：rustup 的 cargo 优先，钉住 Rust 版本，压缩产物体积，
# 超过上限先清理；同一时间只跑一次。
prelude() {
  cat <<EOF
set -uo pipefail
export PATH="\$HOME/$dir/tools/xcodegen-2.46.0/bin:\$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:\$PATH"
export RUSTUP_TOOLCHAIN="$rust" CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_PROFILE_TEST_DEBUG=line-tables-only
ulimit -n 10240
cd ~/$dir/Aeroric
if ! mkdir ~/$dir/.lock 2>/dev/null; then echo "远端正在跑另一次（~/$dir/.lock）"; exit 75; fi
# 测试在临时目录里留下的 aeroric-* 目录：结束时删掉这次运行期间新建的那些。
touch ~/$dir/.lock/started
trap 'find "\${TMPDIR:-/tmp}" -maxdepth 1 -name "aeroric-*" -newer ~/$dir/.lock/started -exec rm -rf {} + 2>/dev/null; rm -rf ~/$dir/.lock' EXIT
if [ -d core/target ]; then
  kb=\$(du -sk core/target | cut -f1)
  if [ "\$kb" -gt \$(( $max_gb * 1024 * 1024 )) ]; then
    echo "core/target 有 \$(( kb / 1024 / 1024 ))G，超过 ${max_gb}G，先清理"
    (cd core && cargo clean -q)
  fi
fi
EOF
}

report() {
  cat <<'EOF'
echo "--- 远端磁盘：$(df -h ~ | tail -1 | awk '{print "剩 " $4 "，已用 " $5}')（测试的临时目录在退出时清理）"
for d in core/target .artifacts macos/AeroricKit/.build; do
  [ -d "$d" ] && echo "    $d $(du -sh "$d" | cut -f1)"
done
EOF
}

verify_script() {
  cat <<'EOF'
log=~/LOGDIR/verify-$(date +%Y%m%d-%H%M%S)
mkdir -p "$log"
(cd core && cargo fmt --check >/dev/null 2>&1); echo "fmt $?"
(cd core && cargo fmt --check 2>&1 | grep '^Diff in' | head -5)
s=$(date +%s)
(cd core && cargo test --workspace --no-fail-fast > "$log/ws.log" 2>&1); echo "ws rc=$? secs=$(( $(date +%s) - s ))"
awk '/test result:/ {p+=$4; f+=$6} END {print "passed", p, "failed", f}' "$log/ws.log"
grep -E '^test .* FAILED|panicked' "$log/ws.log" | head -10
node scripts/native/contracts.mjs --check 2>&1 | tail -1
swift test --package-path macos/AeroricKit > "$log/swift.log" 2>&1; echo "swift rc=$?"
grep -E "Test Suite 'Aeroric[A-Za-z]+\.xctest' (passed|failed)" "$log/swift.log"
pnpm native:build > "$log/build.log" 2>&1; echo "build rc=$?"
tail -1 "$log/build.log"
pnpm -s verify-notes 2>&1 | tail -2
pnpm -s verify-docs 2>&1 | tail -1
echo "日志：$log"
EOF
}

cmd="${1:-}"
[ $# -gt 0 ] && shift
case "$cmd" in
  sync)
    sync
    ;;
  run)
    [ $# -gt 0 ] || { echo "用法：remote.sh run <命令>" >&2; exit 2; }
    sync
    { prelude; printf '%s\n' "$*"; echo 'rc=$?'; report; echo 'exit $rc'; } | remote bash -s
    ;;
  verify)
    sync
    { prelude; verify_script | sed "s#LOGDIR#$dir/logs#"; report; } | remote bash -s
    ;;
  build)
    sync
    { prelude; echo 'pnpm native:build; rc=$?'; report; echo 'exit $rc'; } | remote bash -s
    # 只有远端打包成功才拷回来；本机的旧 .app 整个换掉。
    mkdir -p "$repo/$(dirname "$app_rel")"
    rsync -a --delete -e "ssh $ssh_opts" "$host:$dir/Aeroric/$app_rel/" "$repo/$app_rel/"
    echo "已拷回 $repo/$app_rel"
    ;;
  status)
    { echo "cd ~/$dir/Aeroric 2>/dev/null || exit 0"; report; } | remote bash -s
    ;;
  *)
    sed -n '2,20p' "$0" >&2
    exit 2
    ;;
esac
