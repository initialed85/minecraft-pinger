#!/bin/bash

set -e -x

pushd "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null

cargo build --release

sudo systemctl stop minecraft-pinger.service || true

sudo cp -fv target/release/minecraft_pinger /bin/minecraft_pinger

sudo systemctl restart minecraft-pinger.service
