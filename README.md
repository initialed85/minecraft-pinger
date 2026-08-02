# minecraft-pinger

A simple daemon for pinging a given host or IP via ICMP and via the Minecraft Java 26.2 protocol.

## Usage

### Native

NOTE: Needs Cargo / Rust installed

```shell
git clone https://github.com/initialed85/minecraft-pinger

pushd minecraft-pinger

cargo build --release

# NOTE: sudo is required because of the use of a RAW socket for ICMP
# test it out in the foreground (Ctrl + C to stop); change the SERVER env var as required
sudo bash -c 'SERVER=minecraft.initialed85.cc ./target/release/minecraft_pinger'
```

You can then browse to [localhost:8080](http://localhost:8080) to see the UI.

You can run it natively as a systemd service like this (assuming you've already built it as above):

```shell
sudo cp -fv ./target/release/minecraft_pinger /usr/bin/minecraft_pinger

# change the SERVER env var as required in this file as required
sudo cp -fv ./minecraft-pinger.service /etc/systemd/system/minecraft-pinger.service
sudo systemctl daemon-reload
sudo systemctl restart minecraft-pinger.service
```

Same-same, you can then browse to [localhost:8080](http://localhost:8080) to see the UI.

### Kubernetes

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
      volumes:
        - name: minecraft
          configMap:
            name: minecraft
        - name: shared
          persistentVolumeClaim:
            claimName: minecraft
            readOnly: false
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
  ingressClassName: traefik
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
