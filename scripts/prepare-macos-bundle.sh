#!/bin/sh
set -eu

project_directory=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
tauri_directory="$project_directory/src-tauri"
resource_directory="$tauri_directory/resources"
runtime_directory="$resource_directory/binaries/macos"
library_directory="$runtime_directory/lib"
brew_command=${ROUTEPILOT_BREW_COMMAND:-}
if [ -z "$brew_command" ]; then
  brew_command=$(command -v brew || true)
fi

brew_prefix() {
  if [ -z "$brew_command" ]; then
    echo "RoutePilot build requires Homebrew or explicit ROUTEPILOT_*_LIB_DIR values" >&2
    exit 1
  fi
  "$brew_command" --prefix "$1"
}

openvpn_source=${ROUTEPILOT_OPENVPN_SOURCE:-"$(brew_prefix openvpn)/sbin/openvpn"}
lzo_library_directory=${ROUTEPILOT_LZO_LIB_DIR:-"$(brew_prefix lzo)/lib"}
lz4_library_directory=${ROUTEPILOT_LZ4_LIB_DIR:-"$(brew_prefix lz4)/lib"}
pkcs11_library_directory=${ROUTEPILOT_PKCS11_LIB_DIR:-"$(brew_prefix pkcs11-helper)/lib"}
openssl_library_directory=${ROUTEPILOT_OPENSSL_LIB_DIR:-"$(brew_prefix openssl@3)/lib"}
openssl_library_directory=$(CDPATH= cd -- "$openssl_library_directory" && pwd -P)
openssl_crypto_real="$openssl_library_directory/libcrypto.3.dylib"
codesign_identity=${ROUTEPILOT_CODESIGN_IDENTITY:--}
target_architecture=${ROUTEPILOT_TARGET_ARCH:-$(uname -m)}

if [ ! -x "$openvpn_source" ]; then
  echo "RoutePilot build requires an executable OpenVPN at $openvpn_source" >&2
  exit 1
fi

for dependency in \
  "$lzo_library_directory/liblzo2.2.dylib" \
  "$lz4_library_directory/liblz4.1.dylib" \
  "$pkcs11_library_directory/libpkcs11-helper.1.dylib" \
  "$openssl_library_directory/libssl.3.dylib" \
  "$openssl_crypto_real"
do
  if [ ! -f "$dependency" ]; then
    echo "RoutePilot build dependency is missing: $dependency" >&2
    exit 1
  fi
done

cargo build --release --manifest-path "$tauri_directory/Cargo.toml" --bin routepilot-helper

/bin/mkdir -p "$library_directory"
/usr/bin/install -m 0755 "$tauri_directory/target/release/routepilot-helper" "$resource_directory/routepilot-helper"
/usr/bin/install -m 0755 "$openvpn_source" "$runtime_directory/openvpn"
/usr/bin/install -m 0644 "$lzo_library_directory/liblzo2.2.dylib" "$library_directory/liblzo2.2.dylib"
/usr/bin/install -m 0644 "$lz4_library_directory/liblz4.1.dylib" "$library_directory/liblz4.1.dylib"
/usr/bin/install -m 0644 "$pkcs11_library_directory/libpkcs11-helper.1.dylib" "$library_directory/libpkcs11-helper.1.dylib"
/usr/bin/install -m 0644 "$openssl_library_directory/libssl.3.dylib" "$library_directory/libssl.3.dylib"
/usr/bin/install -m 0644 "$openssl_crypto_real" "$library_directory/libcrypto.3.dylib"

linked_path() {
  /usr/bin/otool -L "$1" | /usr/bin/awk -v library="$2" '
    {
      component_count = split($1, path_components, "/")
      if (path_components[component_count] == library) {
        print $1
        exit
      }
    }
  '
}

openvpn_lzo_link=$(linked_path "$runtime_directory/openvpn" liblzo2.2.dylib)
openvpn_lz4_link=$(linked_path "$runtime_directory/openvpn" liblz4.1.dylib)
openvpn_pkcs11_link=$(linked_path "$runtime_directory/openvpn" libpkcs11-helper.1.dylib)
openvpn_ssl_link=$(linked_path "$runtime_directory/openvpn" libssl.3.dylib)
openvpn_crypto_link=$(linked_path "$runtime_directory/openvpn" libcrypto.3.dylib)
pkcs11_crypto_link=$(linked_path "$library_directory/libpkcs11-helper.1.dylib" libcrypto.3.dylib)
ssl_crypto_link=$(linked_path "$library_directory/libssl.3.dylib" libcrypto.3.dylib)

for linked_dependency in \
  "$openvpn_lzo_link" "$openvpn_lz4_link" "$openvpn_pkcs11_link" \
  "$openvpn_ssl_link" "$openvpn_crypto_link" "$pkcs11_crypto_link" "$ssl_crypto_link"
do
  if [ -z "$linked_dependency" ]; then
    echo "RoutePilot could not resolve a required dynamic-library load path" >&2
    exit 1
  fi
done

/usr/bin/install_name_tool \
  -change "$openvpn_lzo_link" @loader_path/lib/liblzo2.2.dylib \
  -change "$openvpn_lz4_link" @loader_path/lib/liblz4.1.dylib \
  -change "$openvpn_pkcs11_link" @loader_path/lib/libpkcs11-helper.1.dylib \
  -change "$openvpn_ssl_link" @loader_path/lib/libssl.3.dylib \
  -change "$openvpn_crypto_link" @loader_path/lib/libcrypto.3.dylib \
  "$runtime_directory/openvpn"

/usr/bin/install_name_tool -id @loader_path/liblzo2.2.dylib "$library_directory/liblzo2.2.dylib"
/usr/bin/install_name_tool -id @loader_path/liblz4.1.dylib "$library_directory/liblz4.1.dylib"
/usr/bin/install_name_tool \
  -id @loader_path/libpkcs11-helper.1.dylib \
  -change "$pkcs11_crypto_link" @loader_path/libcrypto.3.dylib \
  "$library_directory/libpkcs11-helper.1.dylib"
/usr/bin/install_name_tool \
  -id @loader_path/libssl.3.dylib \
  -change "$ssl_crypto_link" @loader_path/libcrypto.3.dylib \
  "$library_directory/libssl.3.dylib"
/usr/bin/install_name_tool -id @loader_path/libcrypto.3.dylib "$library_directory/libcrypto.3.dylib"

for binary in "$runtime_directory/openvpn" "$library_directory"/*.dylib "$resource_directory/routepilot-helper"
do
  if ! /usr/bin/lipo "$binary" -verify_arch "$target_architecture"; then
    echo "RoutePilot bundle input does not contain architecture $target_architecture: $binary" >&2
    exit 1
  fi
  /usr/bin/codesign --force --sign "$codesign_identity" "$binary"
done

if /usr/bin/otool -L "$runtime_directory/openvpn" "$library_directory"/*.dylib \
  | /usr/bin/grep -E '/(opt/homebrew|usr/local/opt)/' >/dev/null
then
  echo "RoutePilot bundled runtime still references a Homebrew path" >&2
  exit 1
fi
