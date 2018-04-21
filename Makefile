NAME=kubecat
VERSION=$(shell git rev-parse HEAD)
SEMVER_VERSION=$(shell grep version Cargo.toml | awk -F"\"" '{print $$2}' | head -n 1)
REPO=quay.io/babylonhealth

compile:
	docker run \
		-v cargo-cache:/root/.cargo \
		-v "$$PWD:/volume" -w /volume \
		--rm -it clux/muslrust:stable cargo build --release
	cp target/x86_64-unknown-linux-musl/release/shipcat shipcat.x86_64-unknown-linux-musl

build:
	docker build -t $(REPO)/$(NAME):$(VERSION) .

install:
	docker push $(REPO)/$(NAME):$(VERSION)

tag-semver:
	docker tag  $(REPO)/$(NAME):$(VERSION) $(REPO)/$(NAME):$(SEMVER_VERSION)
	docker push $(REPO)/$(NAME):$(SEMVER_VERSION)

tag-latest:
	docker tag  $(REPO)/$(NAME):$(VERSION) $(REPO)/$(NAME):latest
	docker push $(REPO)/$(NAME):latest

doc:
	cargo doc
	xdg-open target/doc/shipcat/index.html

# Package up all built artifacts for ghr to release
#
# releases/
# ├── shipcat.x86_64-apple-darwin.tar.gz
# └── shipcat.x86_64-unknown-linux-musl.tar.gz
releases:
	make release-x86_64-unknown-linux-musl
	make release-x86_64-apple-darwin

# Package a shipcat.$* up with complete script in a standard folder structure:
#
# -rw-r--r-- user/user      5382 2018-04-21 02:43 share/shipcat/shipcat.complete.sh
# -rwxr-xr-x user/user         0 2018-04-21 02:43 bin/shipcat
#
# This should be extractable into /usr/local/ and just work.
release-%:
	mkdir -p releases/$*/bin
	mkdir -p releases/$*/share/shipcat
	cp shipcat.complete.sh releases/$*/share/shipcat
	cp shipcat.$* releases/$*/bin/shipcat
	chmod +x releases/$*/bin/shipcat
	cd releases && tar czf shipcat.$*.tar.gz --transform=s,^$*/,, $$(find $*/ -type f -o -type l)
	tar tvf releases/shipcat.$*.tar.gz
	rm -rf releases/$*/

.PHONY: doc install build compile releases
