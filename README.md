# minecraft-pinger

A simple daemon for pinging a given host or IP via ICMP and via the Minecraft Java 26.2 protocol.

I've (hopefully still) got it running for my own server at these two URLs:

- [minecraft-ping-home.dev.initialed85.cc/](https://minecraft-ping-home.dev.initialed85.cc/)
- [minecraft-ping-perth.dev.initialed85.cc/](https://minecraft-ping-perth.dev.initialed85.cc/)

## Usage

### Native

```shell
pushd minecraft-pinger

# if Linux AMD64
curl -L -O https://github.com/initialed85/minecraft-pinger/releases/download/v0.0.1/minecraft_pinger-linux-amd64

# or, if Linux ARM64
curl -L -O https://github.com/initialed85/minecraft-pinger/releases/download/v0.0.1/minecraft_pinger-linux-arm64

sudo mv minecraft_pinger-linux-* /usr/bin/minecraft_pinger

# test (in foreground, Ctrl + C to quit); note: sudo required because ping uses a raw socket
sudo bash -c 'SERVER=minecraft.initialed85.cc minecraft_pinger'
```

You can then browse to [localhost:8080](http://localhost:8080) to see the UI.

If you want to run it as a systemd service (having already done the above):

```shell
curl -L -O https://raw.githubusercontent.com/initialed85/minecraft-pinger/refs/tags/v0.0.1/minecraft-pinger.service

#
# ensure to edit the SERVER env var as required in "minecraft-pinger.service" first
#

sudo mv minecraft-pinger.service /etc/systemd/system/minecraft-pinger.service
sudo systemctl daemon-reload
sudo systemctl restart minecraft-pinger.service
```

Same-same, you can then browse to [localhost:8080](http://localhost:8080) to see the UI.

### Kubernetes

NOTE: The Docker image has only been built and pushed for Linux AMD64 at the moment.

Kubernetes is a bit like sex, I can't really tell you the best way to f\*ck your wife- but it's something like this (assuming you have cert-manager):

```yaml
---
apiVersion: v1
kind: Namespace
metadata:
  name: minecraft
  labels:
    name: minecraft
---
apiVersion: apps/v1
kind: Deployment
metadata:
  namespace: minecraft
  name: pinger
  labels:
    app: pinger
spec:
  replicas: 1
  selector:
    matchLabels:
      app: pinger
  template:
    metadata:
      labels:
        app: pinger
    spec:
      containers:
        - name: pinger
          image: initialed85/minecraft-pinger:latest
          imagePullPolicy: Always
          env:
            - name: SERVER
              # NOTE: ensure to change this as required
              value: minecraft.initialed85.cc
          ports:
            - containerPort: 8080
---
apiVersion: v1
kind: Service
metadata:
  name: pinger
  namespace: minecraft
spec:
  selector:
    app: pinger
  ports:
    - name: port-1
      protocol: TCP
      port: 8080
      targetPort: 8080
---
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  namespace: minecraft
  name: minecraft-pinger
  annotations:
    cert-manager.io/cluster-issuer: letsencrypt
spec:
  tls:
    - hosts:
        # NOTE: ensure to change this as required
        - minecraft-ping-home.dev.initialed85.cc
      secretName: minecraft-pinger
  rules:
    # NOTE: ensure to change this as required
    - host: minecraft-ping-home.dev.initialed85.cc
      http:
        paths:
          - pathType: Prefix
            path: /
            backend:
              service:
                name: pinger
                port:
                  number: 8080
```
