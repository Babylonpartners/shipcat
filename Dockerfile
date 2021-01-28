FROM alpine:3.7

ENV KUBEVER=1.15.5 \
    HELMVER=2.13.0 \
    KUBEVALVER=0.14.0 \
    VAULTVER=0.11.1 \
    HOME=/config \
    SSL_CERT_DIR=/etc/ssl/certs/

# Install kubectl (see https://aur.archlinux.org/packages/kubectl-bin )
ADD https://storage.googleapis.com/kubernetes-release/release/v${KUBEVER}/bin/linux/amd64/kubectl /usr/local/bin/kubectl

# Install everything
# NB: skipping https://github.com/garethr/kubetest because alpine dylibs fail
RUN set -x && \
    apk update && \
    apk add --no-cache curl ca-certificates findutils make bash jq git python3 unzip && \
    chmod +x /usr/local/bin/kubectl && \
    curl -sSL https://storage.googleapis.com/kubernetes-helm/helm-v${HELMVER}-linux-amd64.tar.gz | tar xz -C /usr/local/bin --strip-components=1 && \
    curl -sSL https://github.com/garethr/kubeval/releases/download/${KUBEVALVER}/kubeval-linux-amd64.tar.gz | tar xvz -C /usr/local/bin && \
    curl -sSL https://releases.hashicorp.com/vault/${VAULTVER}/vault_${VAULTVER}_linux_amd64.zip > vault.zip && \
    unzip vault.zip && mv vault /usr/local/bin && \
    apk del unzip && rm vault.zip

# Create non-root user (alpine)
RUN adduser kubectl -Du 1000 -h /config

# Smoke checks
RUN kubectl version --client
RUN helm version -c
# Note: the old version of Helm uses the old Helm stable charts repo by default.
RUN helm init -c --stable-repo-url https://charts.helm.sh/stable
RUN kubeval --version


# Add core dependencies of validation
ADD requirements.txt ./
RUN apk add --no-cache --virtual virtualbuild libffi-dev g++ python3-dev openssl-dev && \
    pip3 install -r requirements.txt && \
    apk del virtualbuild

# Install shipcat (built for musl outside)
ADD shipcat.x86_64-unknown-linux-musl /usr/local/bin/shipcat

USER kubectl
