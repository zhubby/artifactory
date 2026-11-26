#!/bin/sh
# [INPUT]: 固定资产、签名候选、服务私有路径与环境注册凭证。
# [OUTPUT]: 以专用账户首次安装，或固定私有候选来维护原启动器。
# [POS]: Unix安装与管理员维护入口；保留既有节点身份。
# [PROTOCOL]: Update this header on change, then check AGENTS.md
set -eu
umask 077

fail() { printf '%s\n' "Artifactory: $*" >&2; exit 1; }
enrollment_token=${ARTIFACTORY_ENROLLMENT_TOKEN-}
unset ARTIFACTORY_ENROLLMENT_TOKEN
temporary=
token_file=
maintenance_private=0
cleanup() {
    unset enrollment_token ARTIFACTORY_ENROLLMENT_TOKEN
    if [ -n "$token_file" ]; then sudo -- rm -f -- "$token_file" || true; fi
    if [ -n "$temporary" ]; then
        if [ "$maintenance_private" = 1 ]; then sudo -- rm -rf -- "$temporary"; else rm -rf -- "$temporary"; fi
    fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

download_url='' sha256='' size='' endpoint='' ca_base64='' ca_sha256='' public_keys='' os='' arch=''
service_user='' directory='' binary_directory=''
maintenance=0 maintenance_binary='' maintenance_candidate='' maintenance_manifest='' maintenance_signature='' maintenance_config='' maintenance_stopped=0 maintenance_sha256=''
while [ "$#" -gt 0 ]; do
    case "$1" in
        --maintain-launcher) maintenance=1; shift; continue ;;
        --service-stopped) maintenance_stopped=1; shift; continue ;;
    esac
    [ "$#" -ge 2 ] || fail "Missing value for $1"
    case "$1" in
        --download-url) download_url=$2 ;;
        --sha256) sha256=$2 ;;
        --size) size=$2 ;;
        --endpoint) endpoint=$2 ;;
        --ca-base64) ca_base64=$2 ;;
        --ca-sha256) ca_sha256=$2 ;;
        --public-keys) public_keys=$2 ;;
        --os) os=$2 ;;
        --arch) arch=$2 ;;
        --user) service_user=$2 ;;
        --directory) directory=$2 ;;
        --binary-directory) binary_directory=$2 ;;
        --installed-binary) maintenance_binary=$2 ;;
        --candidate) maintenance_candidate=$2 ;;
        --candidate-sha256) maintenance_sha256=$2 ;;
        --manifest) maintenance_manifest=$2 ;;
        --signature) maintenance_signature=$2 ;;
        --config) maintenance_config=$2 ;;
        *) fail "Unknown option: $1" ;;
    esac
    shift 2
done
if [ "$maintenance" = 1 ]; then
    unset enrollment_token
    for path in "$maintenance_binary" "$maintenance_candidate" "$maintenance_manifest" "$maintenance_signature" "$maintenance_config"; do
        case "$path" in /*) ;; *) fail 'Maintenance requires absolute local file paths.' ;; esac
    done
    case "$maintenance_sha256" in *[!a-f0-9]*) fail 'Invalid signed-release candidate digest.' ;; esac
    [ ${#maintenance_sha256} -eq 64 ] || fail 'The signed release candidate SHA-256 is required.'
    # 服务配置父目录保持私有；管理员检查路径，不放宽服务身份权限。
    for path in "$maintenance_binary" "$maintenance_candidate" "$maintenance_manifest" "$maintenance_signature" "$maintenance_config"; do
        sudo -- test -f "$path" && sudo -- test ! -L "$path" || fail 'Maintenance files must be ordinary existing files.'
    done
    # 在系统临时目录用管理员身份固定副本，再校验真正执行的文件。
    temporary=$(sudo -- mktemp -d '/tmp/artifactory-maintain.XXXXXXXX')
    maintenance_private=1
    sudo -- cp "$maintenance_candidate" "$temporary/maintenance-tool"
    sudo -- chmod 500 "$temporary/maintenance-tool"
    actual=$(sudo -- openssl dgst -sha256 "$temporary/maintenance-tool" | sed 's/^.*= //')
    [ "$actual" = "$maintenance_sha256" ] || fail 'Maintenance candidate SHA-256 mismatch.'
    # 摘要来自已验签的固定发行；新工具先用既有公钥验签，再改动服务。
    if [ "$maintenance_stopped" = 1 ]; then
        sudo -- "$temporary/maintenance-tool" launcher-maintain --config "$maintenance_config" --installed-binary "$maintenance_binary" --candidate "$temporary/maintenance-tool" --manifest "$maintenance_manifest" --signature "$maintenance_signature" --service-stopped
    else
        sudo -- "$temporary/maintenance-tool" launcher-maintain --config "$maintenance_config" --installed-binary "$maintenance_binary" --candidate "$temporary/maintenance-tool" --manifest "$maintenance_manifest" --signature "$maintenance_signature"
    fi
    exit $?
fi
[ -n "$enrollment_token" ] || fail 'ARTIFACTORY_ENROLLMENT_TOKEN is required; generate a new installation command.'
[ ${#enrollment_token} -le 4096 ] || fail 'Invalid enrollment token.'
case "$download_url" in https://?*) ;; *) fail 'The binary download URL must use HTTPS.' ;; esac
case "$download_url" in *[[:space:]]*|*'#'*|*'@'*) fail 'Invalid binary download URL.' ;; esac
case "$sha256$ca_sha256" in *[!a-f0-9]*) fail 'Invalid SHA-256 digest.' ;; esac
[ ${#sha256} -eq 64 ] && [ ${#ca_sha256} -eq 64 ] || fail 'Invalid SHA-256 digest.'
case "$size" in ''|*[!0-9]*|0*) fail 'Invalid binary size.' ;; esac
[ ${#size} -le 9 ] && [ "$size" -le 134217728 ] || fail 'Binary size exceeds 128 MiB.'
[ -n "$endpoint" ] && [ ${#endpoint} -le 300 ] || fail 'Invalid device endpoint.'
[ -z "$(printf '%s' "$endpoint" | LC_ALL=C tr -d 'A-Za-z0-9.:[\]-')" ] || fail 'Invalid device endpoint.'
port=${endpoint##*:}
host=${endpoint%:*}
case "$port" in ''|*[!0-9]*|0*) fail 'The device endpoint requires a TCP port.' ;; esac
[ ${#port} -le 5 ] && [ "$port" -le 65535 ] || fail 'Invalid device port.'
case "$host" in
    \[*\]) [ -n "$(printf '%s' "$host" | tr -d '[]:')" ] || fail 'Invalid IPv6 host.' ;;
    ''|*:*|*[!A-Za-z0-9.-]*) fail 'Invalid device host.' ;;
esac
case "$(uname -s)" in Linux) actual_os=linux ;; Darwin) actual_os=darwin ;; *) fail 'Unsupported platform.' ;; esac
case "$(uname -m)" in x86_64|amd64) actual_arch=amd64 ;; aarch64|arm64) actual_arch=arm64 ;; *) fail 'Unsupported platform architecture.' ;; esac
if [ "$actual_os" = darwin ] && [ "$(sysctl -n hw.optional.arm64 2>/dev/null || true)" = 1 ]; then actual_arch=arm64; fi
[ "$os/$arch" = "$actual_os/$actual_arch" ] || fail "Selected platform $os/$arch does not match $actual_os/$actual_arch."
[ "$(id -u)" != 0 ] || fail 'Run this installer from an ordinary operator account; sudo is requested for installation.'
for command in sudo curl openssl install; do command -v "$command" >/dev/null 2>&1 || fail "Required command missing: $command"; done
if [ -z "$service_user" ]; then
    printf 'Existing dedicated non-administrator service account: ' >/dev/tty
    read -r service_user </dev/tty || fail 'A dedicated service account is required.'
fi
case "$service_user" in ''|*[!A-Za-z0-9_.-]*|-*|root) fail 'Invalid dedicated service account.' ;; esac
uid=$(id -u "$service_user" 2>/dev/null) || fail 'Create the dedicated non-administrator service account first.'
gid=$(id -g "$service_user")
[ "$uid" != 0 ] || fail 'The service account must not be root.'
case " $(id -Gn "$service_user") " in *' sudo '*|*' wheel '*|*' admin '*|*' root '*) fail 'The service account must not belong to an administrator group.' ;; esac
if [ "$os" = linux ]; then
    command -v systemctl >/dev/null 2>&1 || fail 'This installer requires systemd.'
    directory=${directory:-/var/lib/artifactory-emissary}
    binary_directory=${binary_directory:-/usr/local/lib/artifactory-emissary}
else
    directory=${directory:-/Library/Application Support/Artifactory/Emissary/state}
    binary_directory=${binary_directory:-/Library/Application Support/Artifactory/Emissary/bin}
fi
for path in "$directory" "$binary_directory"; do
    case "$path" in /*) ;; *) fail 'Installation paths must be absolute.' ;; esac
    case "$path/" in *'/../'*|*'/./'*|*'//'*) fail 'Installation paths must not contain relative or empty segments.' ;; esac
    [ "$path" != / ] && [ -z "$(printf '%s' "$path" | LC_ALL=C tr -d '[:print:]')" ] || fail 'Invalid installation path.'
done
sudo -v
for path in "$directory" "$binary_directory"; do
    if sudo -- test -e "$path" || sudo -- test -L "$path"; then fail "Installation path already exists; existing identities are preserved: $path"; fi
    parent=${path%/*}
    while [ -n "$parent" ] && [ "$parent" != / ]; do
        if sudo -- test -L "$parent"; then fail "Installation parent is a symbolic link: $parent"; fi
        parent=${parent%/*}
    done
done
if [ "$os" = linux ]; then
    state=$(systemctl show --property=LoadState --value artifactory-emissary.service 2>/dev/null || true)
    [ -z "$state" ] || [ "$state" = not-found ] || fail 'The Emissary service already exists.'
    [ ! -e /etc/systemd/system/artifactory-emissary.service ] || fail 'The Emissary service already exists.'
else
    if launchctl print system/artifactory-emissary >/dev/null 2>&1 || [ -e /Library/LaunchDaemons/artifactory-emissary.plist ]; then fail 'The Emissary service already exists.'; fi
fi

# ---- 下载校验先于任何身份或服务变更 ----
temporary=$(mktemp -d "${TMPDIR:-/tmp}/artifactory-install.XXXXXXXX")
printf '%s' "$ca_base64" | openssl base64 -d -A > "$temporary/ca.pem" || fail 'Invalid CA encoding.'
[ "$(wc -c < "$temporary/ca.pem" | tr -d ' ')" -le 65536 ] || fail 'CA exceeds 64 KiB.'
digest() { openssl dgst -sha256 "$1" | sed 's/^.*= //'; }
[ "$(digest "$temporary/ca.pem")" = "$ca_sha256" ] || fail 'CA SHA-256 mismatch.'
openssl x509 -in "$temporary/ca.pem" -noout >/dev/null 2>&1 || fail 'The supplied CA is not a PEM certificate.'
case "$public_keys" in ''|,*|*,|*,,*|*[!A-Za-z0-9+/=,]*) fail 'Invalid release verification keys.' ;; esac
remaining=$public_keys
while :; do
    key=${remaining%%,*}
    [ ${#key} -eq 44 ] || fail 'Invalid release verification key.'
    printf '%s' "$key" | openssl base64 -d -A > "$temporary/key.bin" || fail 'Invalid release verification key.'
    [ "$(wc -c < "$temporary/key.bin" | tr -d ' ')" -eq 32 ] || fail 'Invalid release verification key.'
    case "$remaining" in *,*) remaining=${remaining#*,} ;; *) break ;; esac
done
status=$(curl --fail --silent --show-error --proto '=https' --tlsv1.2 --connect-timeout 20 --max-time 300 --max-filesize "$size" --output "$temporary/emissary" --write-out '%{http_code}' "$download_url") || fail 'Binary download failed; generate a fresh command if the ticket expired.'
[ "$status" = 200 ] || fail 'Binary download must return HTTP 200 without redirects.'
[ "$(wc -c < "$temporary/emissary" | tr -d ' ')" = "$size" ] || fail 'Binary size mismatch.'
[ "$(digest "$temporary/emissary")" = "$sha256" ] || fail 'Binary SHA-256 mismatch.'

# ---- 启动器归管理员，身份与运行目录归专用账户 ----
binary_parent=$(dirname "$binary_directory")
if ! sudo -- test -d "$binary_parent"; then sudo -- install -d -m 755 "$binary_parent"; fi
sudo -- mkdir -m 755 "$binary_directory"
sudo -- chown 0:0 "$binary_directory"
binary=$binary_directory/artifactory-emissary
sudo -- install -m 755 "$temporary/emissary" "$binary"
sudo -- chown 0:0 "$binary"
data_parent=$(dirname "$directory")
if ! sudo -- test -d "$data_parent"; then sudo -- install -d -m 755 "$data_parent"; fi
sudo -- mkdir -m 700 "$directory"
sudo -- chown "$uid:$gid" "$directory"
sudo -- install -m 600 "$temporary/ca.pem" "$directory/ca.pem"
sudo -- chown "$uid:$gid" "$directory/ca.pem"
config=$directory/emissary.json
token_file=$directory/.enrollment-token
printf '%s' "$enrollment_token" > "$temporary/token"
unset enrollment_token
sudo -- install -m 600 "$temporary/token" "$token_file"
sudo -- chown "$uid:$gid" "$token_file"
rm -f "$temporary/token"
sudo -H -u "$service_user" -- sh -c '
    set -eu
    ARTIFACTORY_ENROLLMENT_TOKEN=$(cat "$1")
    rm -f "$1"
    export ARTIFACTORY_ENROLLMENT_TOKEN
    exec "$2" register --endpoint "$3" --ca "$4" --config "$5" --root "$6"
' sh "$token_file" "$binary" "$endpoint" "$directory/ca.pem" "$config" "$directory/data"
token_file=
sudo -H -u "$service_user" -- "$binary" doctor --config "$config" --root "$directory/data"
sudo -H -u "$service_user" -- "$binary" ota-init --config "$config" --public-keys "$public_keys"
sudo -- "$binary" service --action install --user "$service_user" --config "$config"
sudo -- "$binary" service --action start --config "$config"
printf '%s\n' 'Emissary service started. Check the node console for its authenticated heartbeat.'
