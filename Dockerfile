FROM --platform=linux/amd64 ubuntu:24.04

COPY minecraft_pinger /bin/minecraft_pinger

ENTRYPOINT ["minecraft_pinger"]
