#!/bin/bash

set -e 

rsync -arv --exclude target --exclude .git ./ root@203.29.243.16:minecraft_pinger/

ssh -t root@203.29.243.16 "bash -ic ./minecraft_pinger/_deploy.sh"

scp root@203.29.243.16:minecraft_pinger/target/release/minecraft_pinger .

docker build --platform=linux/amd64 -t initialed85/minecraft-pinger:latest -f ./Dockerfile .

docker image push --platform=linux/amd64 initialed85/minecraft-pinger:latest

kubectl -n minecraft rollout restart deployment/pinger
